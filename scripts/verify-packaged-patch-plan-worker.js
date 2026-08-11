// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const [resourcesRoot, target = 'win32-x64'] = process.argv.slice(2);
if (!resourcesRoot || target !== 'win32-x64') {
    throw new Error('Usage: node scripts/verify-packaged-patch-plan-worker.js <app.asar.unpacked> [win32-x64]');
}

const binary = path.resolve(resourcesRoot, 'native', 'patch-plan-worker', 'bin', target, 'deltamod-patch-plan-worker.exe');
const stats = fs.statSync(binary);
if (!stats.isFile() || stats.size === 0) throw new Error(`Packaged patch-plan worker is invalid: ${binary}`);

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-packaged-plan-'));
try {
    const game = path.join(fixture, 'game');
    const mod = path.join(fixture, 'mod');
    fs.mkdirSync(game);
    fs.mkdirSync(mod);
    fs.writeFileSync(path.join(mod, 'source'), 'patch');
    const input = JSON.stringify({
        schemaVersion: 1,
        gameRoot: game,
        platform: 'win32',
        patches: [{ type: 'override', patch: 'source', to: 'target', mappedTarget: 'target', modName: 'Verifier', modId: 'verifier', modRoot: mod }]
    });
    const result = spawnSync(binary, [], { input, encoding: 'utf8', windowsHide: true, shell: false });
    const response = JSON.parse(result.stdout);
    if (result.status !== 0 || response.ok !== true || response.operationCount !== 1 || response.patchCount !== 1) {
        throw new Error(`Packaged patch-plan worker failed verification: ${binary}`);
    }
} finally {
    fs.rmSync(fixture, { recursive: true, force: true });
}

console.log(`Verified packaged patch-plan worker: ${binary}`);
