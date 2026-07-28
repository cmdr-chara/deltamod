const path = require('path');
const { spawnSync } = require('child_process');
const {
    loadProvenance,
    targetForCurrentPlatform,
    verifyInstallation
} = require('./lib/g3mtool-provenance');

const root = path.resolve(__dirname, '..');
const provenance = loadProvenance(root);

if (process.argv.includes('--manifest-only')) {
    console.log('G3MTool source, license, release URLs, and checksums are pinned.');
    process.exit(0);
}

const targetName = targetForCurrentPlatform();
const executable = verifyInstallation(root, provenance, targetName);
const smoke = spawnSync(executable, ['--help'], {
    encoding: 'utf8',
    timeout: 60_000,
    windowsHide: true
});
if (smoke.error) throw new Error(`Could not execute verified G3MTool: ${smoke.error.message}`);
if (smoke.status !== 0 || !`${smoke.stdout}\n${smoke.stderr}`.includes('patch')) {
    throw new Error(`Verified G3MTool failed its CLI smoke test with exit code ${smoke.status}.`);
}

console.log(`G3MTool ${targetName} provenance, checksums, legal notices, and CLI smoke test verified.`);
