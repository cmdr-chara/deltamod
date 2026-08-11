// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const [resourcesRoot, target = `${process.platform}-${process.arch}`] = process.argv.slice(2);
if (!resourcesRoot) {
    throw new Error('Usage: node scripts/verify-packaged-hash-worker.js <app.asar.unpacked> [platform-arch]');
}

const executable = target.startsWith('win32-') ? 'deltamod-hash-worker.exe' : 'deltamod-hash-worker';
const binary = path.resolve(resourcesRoot, 'native', 'hash-worker', 'bin', target, executable);
const stats = fs.statSync(binary);
if (!stats.isFile() || stats.size === 0) throw new Error(`Packaged hash worker is invalid: ${binary}`);
if (!target.startsWith('win32-') && (stats.mode & 0o111) === 0) {
    throw new Error(`Packaged hash worker is not executable: ${binary}`);
}

console.log(`Verified packaged hash worker: ${binary}`);
