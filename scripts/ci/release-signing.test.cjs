// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '../..');
const release = fs.readFileSync(path.join(root, '.github/workflows/tauri-release.yml'), 'utf8');
const ci = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
const updater = { TAURI_SIGNING_PRIVATE_KEY: 'test-key', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'test-password' };
const windows = { WINDOWS_CERTIFICATE: 'test-pfx', WINDOWS_CERTIFICATE_PASSWORD: 'test-pfx-password' };
const macos = Object.fromEntries([
    'APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'KEYCHAIN_PASSWORD',
    'APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID'
].map(name => [name, `test-${name}`]));

function step(workflow, name) {
    const marker = `      - name: ${name}\n`;
    const start = workflow.indexOf(marker);
    assert.notEqual(start, -1, `Missing workflow step: ${name}`);
    const tail = workflow.slice(start + marker.length);
    const end = tail.search(/^      - /m);
    return end === -1 ? tail : tail.slice(0, end);
}

function script(block, substitutions = {}) {
    const marker = '        run: |\n';
    const start = block.indexOf(marker);
    assert.notEqual(start, -1, 'Expected a literal shell script');
    let source = block.slice(start + marker.length).replace(/^          /gm, '');
    for (const [expression, value] of Object.entries(substitutions)) {
        source = source.replaceAll(`\${{ ${expression} }}`, value);
    }
    assert.ok(!source.includes('${{'), 'All Actions expressions must be substituted before execution');
    return source;
}

function runScript(source, environment = {}) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-signing-test-'));
    try {
        const output = path.join(dir, 'outputs');
        const result = spawnSync('bash', ['-c', source], {
            cwd: root,
            encoding: 'utf8',
            timeout: 5000,
            // Do not inherit real signing credentials into regression tests.
            env: { PATH: process.env.PATH, RUNNER_TEMP: dir, GITHUB_OUTPUT: output, GITHUB_ENV: path.join(dir, 'env'), ...environment }
        });
        assert.ifError(result.error);
        const values = fs.existsSync(output) ? fs.readFileSync(output, 'utf8') : '';
        const config = path.join(dir, 'deltamod-platform-signing.json');
        return { ...result, values, config: fs.existsSync(config) ? JSON.parse(fs.readFileSync(config, 'utf8')) : null };
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
    }
}

const signing = script(step(release, 'Require updater signing and select optional publisher signing'));

for (const [name, credentials, expected] of [
    ['updater only', updater, 'windows_signing=false\nmacos_signing=false\n'],
    ['Windows publisher', { ...updater, ...windows }, 'windows_signing=true\nmacos_signing=false\n'],
    ['Apple publisher', { ...updater, ...macos }, 'windows_signing=false\nmacos_signing=true\n'],
    ['both publishers', { ...updater, ...windows, ...macos }, 'windows_signing=true\nmacos_signing=true\n']
]) {
    test(`release credentials: ${name}`, () => {
        const result = runScript(signing, credentials);
        assert.equal(result.status, 0, result.stderr);
        assert.equal(result.values, expected);
        for (const secret of Object.values(credentials)) assert.ok(!(result.stdout + result.stderr).includes(secret));
    });
}

test('updater credentials are mandatory even with both publisher certificates', () => {
    const result = runScript(signing, { ...windows, ...macos });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /TAURI_SIGNING_PRIVATE_KEY/);
});

test('partially configured publishers fail instead of silently downgrading', () => {
    for (const credentials of [{ ...updater, WINDOWS_CERTIFICATE: 'partial' }, { ...updater, APPLE_ID: 'partial' }]) {
        const result = runScript(signing, credentials);
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /Incomplete .* publisher signing/);
    }
});

test('CI selects stable with updater credentials and no paid certificates', () => {
    const source = script(step(ci, 'Select signed publication or unsigned release validation'), {
        'steps.candidate.outputs.explicit_stable': 'true'
    });
    const result = runScript(source, updater);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.values, 'publish=true\n');
});

test('explicit stable requests never downgrade when the updater key is missing', () => {
    const block = step(ci, 'Select signed publication or unsigned release validation');
    const explicit = runScript(script(block, { 'steps.candidate.outputs.explicit_stable': 'true' }));
    assert.notEqual(explicit.status, 0);
    assert.match(explicit.stderr, /Stable release blocked/);
    const ordinary = runScript(script(block, { 'steps.candidate.outputs.explicit_stable': 'false' }));
    assert.equal(ordinary.status, 0);
    assert.equal(ordinary.values, 'publish=false\n');
});

test('ad-hoc macOS signing does not disable signed updater artifacts', () => {
    const block = step(release, 'Prepare platform signing configuration');
    const result = runScript(script(block, {
        'needs.validate.outputs.unsigned_preview': 'false',
        'needs.validate.outputs.macos_signing': 'false'
    }), { RUNNER_OS: 'macOS' });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.config, { bundle: { macOS: { signingIdentity: '-' } } });
    const preview = runScript(script(block, {
        'needs.validate.outputs.unsigned_preview': 'true',
        'needs.validate.outputs.macos_signing': ''
    }), { RUNNER_OS: 'macOS' });
    assert.equal(preview.status, 0);
    assert.deepEqual(preview.config, { bundle: { createUpdaterArtifacts: false } });
});

test('publisher checks depend on publisher credentials, not the stable channel', () => {
    for (const [name, platform] of [
        ['Import Windows publisher certificate', 'windows'],
        ['Verify Windows publisher signatures', 'windows'],
        ['Sign and verify Deltamod-themed setup shell', 'windows'],
        ['Import Apple Developer ID certificate', 'macos'],
        ['Verify macOS signature and notarization', 'macos']
    ]) {
        assert.ok(step(release, name).includes(`needs.validate.outputs.${platform}_signing == 'true'`));
    }
    assert.match(step(release, 'Build signed update-capable bundle'), /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/);
    assert.match(step(release, 'Prepare signed updater metadata'), /generate-tauri-updater-manifest\.js/);
    assert.match(step(release, 'Disclose release signing status'), /SmartScreen/);
    assert.match(step(release, 'Disclose release signing status'), /Gatekeeper/);
});

test('stable is Latest, previews remain prereleases, all package jobs still gate publication', () => {
    assert.match(release, /publish:\n    needs: \[validate, package, third-party-source\]/);
    assert.match(step(release, 'Publish stable signed release'), /--latest --prerelease=false/);
    assert.match(step(release, 'Publish unsigned Tauri preview'), /--prerelease\s/);
    assert.match(ci, /gh workflow run tauri-release\.yml --ref "\$TAG" -f release_mode=stable/);
});

test('branded Windows setup downloads the exact asset staged for the release', () => {
    const builder = fs.readFileSync(path.join(root, 'scripts/build-installer.js'), 'utf8');
    const template = /const releaseUrl = `([^`]+)`/.exec(builder)?.[1];
    assert.ok(template);
    const url = template.replaceAll('${packageInfo.version}', '2.0.23').replaceAll('${windowsTarget}', 'x86_64-pc-windows-msvc');
    assert.equal(url, 'https://github.com/cmdr-chara/deltamod/releases/download/community-v2.0.23/Deltamod-Community_2.0.23_x86_64-pc-windows-msvc.exe');
    assert.match(step(release, 'Stage uniquely named updater release asset'), /Deltamod-Community_\$\{version\}_\$\{BUILD_TARGET\}\$\{extension\}/);
});
