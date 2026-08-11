// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const workspace = path.join(root, 'native');
const crate = path.join(workspace, 'hash-worker');
const cargo = process.env.CARGO || (process.platform === 'win32'
    ? path.join(process.env.USERPROFILE, '.cargo', 'bin', 'cargo.exe')
    : 'cargo');
const result = spawnSync(cargo, ['build', '--release', '--locked', '--manifest-path', path.join(workspace, 'Cargo.toml'), '--package', 'deltamod-hash-worker'], {
    cwd: root,
    stdio: 'inherit'
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status);

const executable = process.platform === 'win32' ? 'deltamod-hash-worker.exe' : 'deltamod-hash-worker';
const destination = path.join(crate, 'bin', `${process.platform}-${process.arch}`);
fs.mkdirSync(destination, { recursive: true });
fs.copyFileSync(path.join(workspace, 'target', 'release', executable), path.join(destination, executable));
