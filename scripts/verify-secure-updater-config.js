// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const configPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const releaseWorkflowPath = path.join(root, '.github', 'workflows', 'tauri-release.yml');
const EXPECTED_ENDPOINT = 'https://github.com/cmdr-chara/deltamod/releases/latest/download/latest.json';

function isRecord(value) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function validateUpdaterConfig(config) {
    const errors = [];
    const bundle = isRecord(config?.bundle) ? config.bundle : {};
    const updater = isRecord(config?.plugins?.updater) ? config.plugins.updater : null;
    const enabled = bundle.createUpdaterArtifacts === true;

    if (updater?.dangerousInsecureTransportProtocol === true) {
        errors.push('plugins.updater.dangerousInsecureTransportProtocol must never be enabled.');
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
    return errors;
}

function verify(config, workflow) {
    const result = validateUpdaterConfig(config);
    const errors = [...result.errors, ...validateReleaseWorkflow(workflow, result.enabled)];
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
        'run: publish latest.json'
    ].join('\n');
    assert.deepEqual(verify(safeConfig, safeWorkflow), { enabled: true, errors: [] });

    const disabled = verify({ bundle: { active: true, createUpdaterArtifacts: false } }, '');
    assert.equal(disabled.enabled, false);
    assert.deepEqual(disabled.errors, []);

    const insecure = JSON.parse(JSON.stringify(safeConfig));
    insecure.plugins.updater.endpoints = ['http://example.invalid/latest.json'];
    insecure.plugins.updater.dangerousInsecureTransportProtocol = true;
    insecure.plugins.updater.pubkey = 'BEGIN PRIVATE KEY';
    const insecureResult = verify(insecure, '');
    assert.ok(insecureResult.errors.some(error => error.includes('HTTPS')));
    assert.ok(insecureResult.errors.some(error => error.includes('private signing material')));
    assert.ok(insecureResult.errors.some(error => error.includes('dangerousInsecureTransportProtocol')));
    assert.ok(insecureResult.errors.some(error => error.includes('GitHub Actions secrets')));
}

function main() {
    if (process.argv.includes('--self-test')) {
        selfTest();
        console.log('Secure updater policy self-test passed.');
        return;
    }

    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    const workflow = fs.readFileSync(releaseWorkflowPath, 'utf8');
    const result = verify(config, workflow);
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
    EXPECTED_ENDPOINT,
    validateUpdaterConfig,
    validateReleaseWorkflow,
    verify
};
