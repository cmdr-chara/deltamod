// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

if (process.platform !== 'win32' || process.arch !== 'x64') {
    throw new Error('The patch-plan worker is packaged only for Windows x64.');
}

const root = path.resolve(__dirname, '..');
const workspace = path.join(root, 'native');
const crate = path.join(workspace, 'patch-plan-worker');
const cargo = process.env.CARGO || path.join(process.env.USERPROFILE, '.cargo', 'bin', 'cargo.exe');
const result = spawnSync(cargo, ['build', '--release', '--locked', '--manifest-path', path.join(workspace, 'Cargo.toml'), '--package', 'deltamod-patch-plan-worker'], {
    cwd: root,
    stdio: 'inherit'
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status);

const executable = 'deltamod-patch-plan-worker.exe';
const destination = path.join(crate, 'bin', 'win32-x64');
fs.mkdirSync(destination, { recursive: true });
fs.copyFileSync(path.join(workspace, 'target', 'release', executable), path.join(destination, executable));
