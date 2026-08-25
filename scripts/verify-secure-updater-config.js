// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const configPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const releaseWorkflowPath = path.join(root, '.github', 'workflows', 'tauri-release.yml');
const platformConfigPaths = {
    windows: path.join(root, 'src-tauri', 'tauri.windows.conf.json'),
    macos: path.join(root, 'src-tauri', 'tauri.macos.conf.json'),
    linux: path.join(root, 'src-tauri', 'tauri.linux.conf.json')
};
const EXPECTED_ENDPOINT = 'https://github.com/cmdr-chara/deltamod/releases/latest/download/latest.json';
const DANGEROUS_UPDATER_FLAGS = [
    'dangerousInsecureTransportProtocol',
    'dangerousAcceptInvalidCerts',
    'dangerousAcceptInvalidHostnames'
];

function isRecord(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function validateUpdaterConfig(config) {
    const errors = [];
    const bundle = isRecord(config?.bundle) ? config.bundle : {};
    const updater = isRecord(config?.plugins?.updater) ? config.plugins.updater : null;
    const enabled = bundle.createUpdaterArtifacts === true;

    for (const flag of DANGEROUS_UPDATER_FLAGS) {
        if (updater?.[flag] === true) {
            errors.push(`plugins.updater.${flag} must never be enabled.`);
        }
    }

    if (!enabled) {
        return { enabled: false, errors };
    }

    if (bundle.active !== true) {
        errors.push('bundle.active must be true before signed updater artifacts can be enabled.');
    }
    if (!updater) {
        errors.push('plugins.updater must be configured before updater artifacts are enabled.');
        return { enabled: true, errors };
    }

    const pubkey = typeof updater.pubkey === 'string' ? updater.pubkey.trim() : '';
    if (!pubkey) {
        errors.push('plugins.updater.pubkey must contain the updater public key.');
    } else {
        if (pubkey.length > 16 * 1024) {
            errors.push('plugins.updater.pubkey is unexpectedly large.');
        }
        if (/PRIVATE[ _-]?KEY|TAURI_SIGNING_PRIVATE_KEY|BEGIN [^-]*PRIVATE KEY/i.test(pubkey)) {
            errors.push('plugins.updater.pubkey appears to contain private signing material.');
        }
        if (/^(?:file:|\.\.?[\\/]|[A-Za-z]:[\\/])/i.test(pubkey)) {
            errors.push('plugins.updater.pubkey must contain public-key material, not a filesystem path.');
        }
    }

    const endpoints = Array.isArray(updater.endpoints) ? updater.endpoints : [];
    if (endpoints.length !== 1) {
        errors.push('plugins.updater.endpoints must contain exactly the canonical stable endpoint.');
    }

    for (const endpoint of endpoints) {
        if (typeof endpoint !== 'string' || endpoint.length === 0 || endpoint.length > 2048) {
            errors.push('Updater endpoints must be non-empty bounded strings.');
            continue;
        }
        if (/\p{Cc}/u.test(endpoint)) {
            errors.push('Updater endpoints must not contain control characters.');
            continue;
        }
        let parsed;
        try {
            parsed = new URL(endpoint);
        } catch {
            errors.push(`Updater endpoint is not a valid URL: ${endpoint}`);
            continue;
        }
        if (parsed.protocol !== 'https:') {
            errors.push(`Updater endpoint must use HTTPS: ${endpoint}`);
        }
        if (parsed.username || parsed.password) {
            errors.push(`Updater endpoint must not contain credentials: ${endpoint}`);
        }
        if (parsed.hash || parsed.search) {
            errors.push(`Updater endpoint must not contain query or fragment data: ${endpoint}`);
        }
        if (parsed.href !== EXPECTED_ENDPOINT) {
            errors.push(`Updater endpoint must be exactly ${EXPECTED_ENDPOINT}.`);
        }
    }

    return { enabled: true, errors };
}

function validateReleaseWorkflow(workflow, updaterEnabled) {
    if (!updaterEnabled) return [];
    const errors = [];
    const secretReference = /TAURI_SIGNING_PRIVATE_KEY\s*:\s*\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY\s*\}\}/;
    if (!secretReference.test(workflow)) {
        errors.push('Stable release workflow must source TAURI_SIGNING_PRIVATE_KEY from GitHub Actions secrets.');
    }
    if (/TAURI_SIGNING_PRIVATE_KEY\s*:\s*(?!\$\{\{\s*secrets\.)[^\s#][^\r\n]*/.test(workflow)) {
        errors.push('Stable release workflow must never embed TAURI_SIGNING_PRIVATE_KEY directly.');
    }
    if (!/latest\.json/.test(workflow)) {
        errors.push('Stable release workflow must publish the signed latest.json updater manifest.');
    }
    if (!/\.sig/.test(workflow)) {
        errors.push('Stable release workflow must collect Tauri signature files.');
    }
    return errors;
}

function validatePlatformBoundary(platforms) {
    const errors = [];
    for (const platform of ['windows', 'macos']) {
        if (platforms?.[platform]?.bundle?.createUpdaterArtifacts !== true) {
            errors.push(`${platform} must enable signed updater artifacts.`);
        }
    }
    if (platforms?.linux?.bundle?.createUpdaterArtifacts === true) {
        errors.push('Linux .deb releases must not enable automatic updater artifacts.');
    }
    return errors;
}

function verify(config, workflow, platforms = null) {
    const platformEnabled = platforms && ['windows', 'macos']
        .some(platform => platforms?.[platform]?.bundle?.createUpdaterArtifacts === true);
    const effective = platformEnabled
        ? { ...config, bundle: { ...config.bundle, createUpdaterArtifacts: true } }
        : config;
    const result = validateUpdaterConfig(effective);
    const errors = [
        ...result.errors,
        ...(platforms && config?.bundle?.createUpdaterArtifacts !== false
            ? ['Base configuration must keep updater artifacts disabled for Linux .deb builds.']
            : []),
        ...(platforms ? validatePlatformBoundary(platforms) : []),
        ...validateReleaseWorkflow(workflow, result.enabled)
    ];
    return { enabled: result.enabled, errors };
}

function selfTest() {
    const safeConfig = {
        bundle: { active: true, createUpdaterArtifacts: true },
        plugins: {
            updater: {
                pubkey: 'RWQexamplePublicKeyOnly',
                endpoints: [EXPECTED_ENDPOINT]
            }
        }
    };
    const safeWorkflow = [
        'env:',
        '  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}',
        'run: publish latest.json and collect .sig files'
    ].join('\n');
    assert.deepEqual(verify(safeConfig, safeWorkflow), { enabled: true, errors: [] });

    const platformSafe = JSON.parse(JSON.stringify(safeConfig));
    platformSafe.bundle.createUpdaterArtifacts = false;
    const platforms = {
        windows: { bundle: { createUpdaterArtifacts: true } },
        macos: { bundle: { createUpdaterArtifacts: true } }
    };
    assert.deepEqual(verify(platformSafe, safeWorkflow, platforms), {
        enabled: true,
        errors: []
    });
    const linuxEnabled = { ...platforms, linux: { bundle: { createUpdaterArtifacts: true } } };
    assert.ok(verify(platformSafe, safeWorkflow, linuxEnabled).errors
        .some(error => error.includes('Linux .deb')));
    assert.ok(verify(safeConfig, safeWorkflow, platforms).errors
        .some(error => error.includes('Base configuration')));

    const disabled = verify({ bundle: { active: true, createUpdaterArtifacts: false } }, '');
    assert.equal(disabled.enabled, false);
    assert.deepEqual(disabled.errors, []);

    const insecure = JSON.parse(JSON.stringify(safeConfig));
    insecure.plugins.updater.endpoints = ['http://user:pass@example.invalid/latest.json?channel=stable#unsafe'];
    insecure.plugins.updater.dangerousInsecureTransportProtocol = true;
    insecure.plugins.updater.dangerousAcceptInvalidCerts = true;
    insecure.plugins.updater.dangerousAcceptInvalidHostnames = true;
    insecure.plugins.updater.pubkey = 'BEGIN PRIVATE KEY';
    const insecureResult = verify(insecure, '');
    assert.ok(insecureResult.errors.some(error => error.includes('HTTPS')));
    assert.ok(insecureResult.errors.some(error => error.includes('credentials')));
    assert.ok(insecureResult.errors.some(error => error.includes('query or fragment')));
    assert.ok(insecureResult.errors.some(error => error.includes('private signing material')));
    for (const flag of DANGEROUS_UPDATER_FLAGS) {
        assert.ok(insecureResult.errors.some(error => error.includes(flag)));
    }
    assert.ok(insecureResult.errors.some(error => error.includes('GitHub Actions secrets')));

    const wrongEndpoint = JSON.parse(JSON.stringify(safeConfig));
    wrongEndpoint.plugins.updater.endpoints = [
        'https://github.com/cmdr-chara/deltamod/releases/latest/download/not-latest.json'
    ];
    assert.ok(verify(wrongEndpoint, safeWorkflow).errors.some(error => error.includes('exactly')));
}

function main() {
    if (process.argv.includes('--self-test')) {
        selfTest();
        console.log('Secure updater policy self-test passed.');
        return;
    }

    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    const platforms = Object.fromEntries(Object.entries(platformConfigPaths)
        .filter(([, file]) => fs.existsSync(file))
        .map(([platform, file]) => [platform, JSON.parse(fs.readFileSync(file, 'utf8'))]));
    const workflow = fs.readFileSync(releaseWorkflowPath, 'utf8');
    const result = verify(config, workflow, platforms);
    if (result.errors.length > 0) {
        for (const error of result.errors) console.error(`secure-updater: ${error}`);
        process.exitCode = 1;
        return;
    }
    console.log(result.enabled
        ? 'Secure updater policy passed: signed updater configuration is complete.'
        : 'Secure updater policy passed: automatic updates remain safely disabled.');
}

if (require.main === module) main();

module.exports = {
    DANGEROUS_UPDATER_FLAGS,
    EXPECTED_ENDPOINT,
    validatePlatformBoundary,
    validateUpdaterConfig,
    validateReleaseWorkflow,
    verify
};
