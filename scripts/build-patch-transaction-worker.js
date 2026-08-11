const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('The patch transaction worker is packaged only for Windows x64.');
const root = path.resolve(__dirname, '..');
const workspace = path.join(root, 'native');
const cargo = process.env.CARGO || path.join(process.env.USERPROFILE, '.cargo', 'bin', 'cargo.exe');
const result = spawnSync(cargo, ['build', '--release', '--locked', '--manifest-path', path.join(workspace, 'Cargo.toml'), '--package', 'deltamod-patch-transaction-worker'], { cwd: root, stdio: 'inherit' });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status);
const destination = path.join(workspace, 'patch-transaction-worker', 'bin', 'win32-x64');
fs.mkdirSync(destination, { recursive: true });
fs.copyFileSync(path.join(workspace, 'target', 'release', 'deltamod-patch-transaction-worker.exe'), path.join(destination, 'deltamod-patch-transaction-worker.exe'));
