// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const [resourcesRoot, target = 'win32-x64'] = process.argv.slice(2);
if (!resourcesRoot || !target.startsWith('win32-')) {
    throw new Error('Usage: node scripts/verify-packaged-security-worker.js <app.asar.unpacked> [win32-arch]');
}

const binary = path.resolve(resourcesRoot, 'native', 'security-worker', 'bin', target, 'deltamod-security-worker.exe');
const stats = fs.statSync(binary);
if (!stats.isFile() || stats.size === 0) throw new Error(`Packaged security worker is invalid: ${binary}`);

const fixture = fs.mkdtempSync(path.join(require('os').tmpdir(), 'deltamod-packaged-security-'));
try {
    fs.writeFileSync(path.join(fixture, 'file'), 'verified');
    const result = spawnSync(binary, [fixture, '10', '1024', '4'], { encoding: 'utf8', windowsHide: true, shell: false });
    const response = JSON.parse(result.stdout);
    if (result.status !== 0 || response.ok !== true || response.fileCount !== 1 || response.expandedBytes !== 8) {
        throw new Error(`Packaged security worker failed verification: ${binary}`);
    }
} finally {
    fs.rmSync(fixture, { recursive: true, force: true });
}

console.log(`Verified packaged security worker: ${binary}`);
