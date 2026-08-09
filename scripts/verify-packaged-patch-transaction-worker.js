const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const [resourcesRoot, target = 'win32-x64'] = process.argv.slice(2);
if (!resourcesRoot || target !== 'win32-x64') throw new Error('Usage: node scripts/verify-packaged-patch-transaction-worker.js <app.asar.unpacked> [win32-x64]');
const binary = path.resolve(resourcesRoot, 'native', 'patch-transaction-worker', 'bin', target, 'deltamod-patch-transaction-worker.exe');
if (!fs.statSync(binary).isFile()) throw new Error(`Packaged patch transaction worker is invalid: ${binary}`);
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-packaged-transaction-'));
try {
    const input = JSON.stringify({ action: 'validate', game_root: root, journal: { schemaVersion: 1, transactionId: '123-456', state: 'patching', operations: [] } });
    const result = spawnSync(binary, [], { input, encoding: 'utf8', windowsHide: true, shell: false });
    const response = JSON.parse(result.stdout);
    if (result.status !== 0 || response.ok !== true) throw new Error(`Packaged patch transaction worker failed verification: ${binary}`);
} finally { fs.rmSync(root, { recursive: true, force: true }); }
console.log(`Verified packaged patch transaction worker: ${binary}`);
