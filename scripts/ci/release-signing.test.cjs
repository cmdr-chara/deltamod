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

test('stable release requires only Tauri updater signing credentials', () => {
    const signing = script(step(release, 'Require updater signing credentials'));
    assert.equal(runScript(signing, updater).status, 0);
    const missing = runScript(signing);
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /TAURI_SIGNING_PRIVATE_KEY/);
});

test('platform publisher credential paths are absent', () => {
    assert.doesNotMatch(release, /CERTIFICATE|KEYCHAIN_PASSWORD|APPLE_ID|APPLE_PASSWORD|APPLE_TEAM_ID/);
    assert.doesNotMatch(release, /Import Windows publisher|Import Apple Developer ID|Developer ID-signed/);
});

test('CI selects stable with updater credentials only', () => {
    const source = script(step(ci, 'Select signed publication or unsigned release validation'), {
        'steps.candidate.outputs.explicit_stable': 'true'
    });
    const result = runScript(source, updater);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.values, 'publish=true\n');
});

test('explicit stable requests never downgrade when updater signing is missing', () => {
    const block = step(ci, 'Select signed publication or unsigned release validation');
    const explicit = runScript(script(block, { 'steps.candidate.outputs.explicit_stable': 'true' }));
    assert.notEqual(explicit.status, 0);
    assert.match(explicit.stderr, /Stable release blocked/);
});

test('stable macOS uses ad-hoc app signing while retaining updater artifacts', () => {
    const block = step(release, 'Prepare platform signing configuration');
    const stable = runScript(script(block, { 'needs.validate.outputs.unsigned_preview': 'false' }), { RUNNER_OS: 'macOS' });
    assert.equal(stable.status, 0, stable.stderr);
    assert.deepEqual(stable.config, { bundle: { macOS: { signingIdentity: '-' } } });
    const preview = runScript(script(block, { 'needs.validate.outputs.unsigned_preview': 'true' }), { RUNNER_OS: 'macOS' });
    assert.equal(preview.status, 0);
    assert.deepEqual(preview.config, { bundle: { createUpdaterArtifacts: false } });
});

test('Windows and macOS stable builders carry only updater signing inputs', () => {
    for (const name of ['Build Windows update-capable bundle', 'Build ad-hoc macOS update-capable bundle']) {
        const block = step(release, name);
        assert.match(block, /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/);
        assert.doesNotMatch(block, /CERTIFICATE|KEYCHAIN|NOTAR|APPLE_ID|APPLE_PASSWORD|APPLE_TEAM_ID/);
    }
    assert.match(step(release, 'Build ad-hoc macOS update-capable bundle'), /APPLE_SIGNING_IDENTITY: '-'/);
});

test('stable packages verify absence of publisher identity', () => {
    assert.match(step(release, 'Verify Windows packages are publisher-unsigned'), /Get-AuthenticodeSignature/);
    assert.match(step(release, 'Verify macOS packages use no Developer ID'), /Developer ID Application/);
    assert.match(step(release, 'Verify branded setup is publisher-unsigned'), /NotSigned/);
});

test('stable is Latest and previews remain prereleases', () => {
    assert.match(release, /publish:\n    needs: \[validate, package, third-party-source\]/);
    assert.match(step(release, 'Publish stable updater-signed release'), /--latest --prerelease=false/);
    assert.match(step(release, 'Publish unsigned Tauri preview'), /--prerelease\s/);
    assert.match(ci, /gh workflow run tauri-release\.yml --ref "\$TAG" -f release_mode=stable/);
});
