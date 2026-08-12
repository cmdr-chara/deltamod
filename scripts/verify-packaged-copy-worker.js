// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { runNativeStagedCopy } = require('../node/storage/NativeStagedCopy');

const [resourcesRoot, target = 'win32-x64'] = process.argv.slice(2);
if (!resourcesRoot || target !== 'win32-x64') {
    throw new Error('Usage: node scripts/verify-packaged-copy-worker.js <app.asar.unpacked> [win32-x64]');
}
const binary = path.resolve(resourcesRoot, 'native', 'copy-worker', 'bin', target, 'deltamod-copy-worker.exe');
const stats = fs.statSync(binary);
if (!stats.isFile() || stats.size === 0) throw new Error(`Packaged copy worker is invalid: ${binary}`);

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-packaged-copy-'));
(async () => {
    try {
        const source = path.join(fixture, 'source');
        const destination = path.join(fixture, 'destination');
        fs.mkdirSync(source);
        fs.writeFileSync(path.join(source, 'file'), 'verified');
        const result = await runNativeStagedCopy({
            source,
            destination,
            operationId: 'packaged-verifier',
            retries: 1,
            availableBytes: null,
            sidecarPath: binary
        });
        if (result.fileCount !== 1 || result.totalBytes !== 8 || fs.readFileSync(path.join(destination, 'file'), 'utf8') !== 'verified') {
            throw new Error(`Packaged copy worker failed verification: ${binary}`);
        }
        console.log(`Verified packaged copy worker: ${binary}`);
    } finally {
        fs.rmSync(fixture, { recursive: true, force: true });
    }
})().catch(error => {
    console.error(error);
    process.exitCode = 1;
});
