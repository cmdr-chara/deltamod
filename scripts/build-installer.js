'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const packageInfo = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const target = process.env.TAURI_BUILD_TARGET || process.env.RUST_TARGET || '';
const windowsTarget = target || 'x86_64-pc-windows-msvc';

if (process.platform !== 'win32' || (target && target !== 'x86_64-pc-windows-msvc')) {
    throw new Error('The branded Deltamod setup currently targets Windows x64.');
}

function run(command, args, env = process.env) {
    const useShell = process.platform === 'win32' && (command === 'npm' || command === 'npx');
    const executable = useShell ? (process.env.ComSpec || 'cmd.exe') : command;
    const commandArgs = useShell
        ? ['/d', '/s', '/c', [command, ...args].map(value => {
            const text = String(value);
            return /[\s"&^|<>]/.test(text) ? `"${text.replace(/"/g, '\\"')}"` : text;
        }).join(' ')]
        : args;
    const result = spawnSync(executable, commandArgs, {
        cwd: root,
        env,
        stdio: 'inherit',
        windowsHide: true
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${result.status}.`);
}

const targetDir = path.join(root, 'src-tauri', 'target-installer');
const releaseUrl = `https://github.com/cmdr-chara/deltamod/releases/download/community-v${packageInfo.version}/Deltamod.Community_${packageInfo.version}_x64-setup.exe`;
const env = {
    ...process.env,
    CARGO_TARGET_DIR: targetDir,
    DELTAMOD_INSTALLER_MODE: '1',
    DELTAMOD_INSTALLER_ASSET_URL: releaseUrl
};

run('npm', ['run', 'build:boot'], env);
const args = ['tauri', 'build', '--config', 'src-tauri/tauri.conf.json', '--no-bundle'];
if (target) args.push('--target', target);
run('npx', args, env);

const candidates = [
    path.join(targetDir, windowsTarget, 'release', 'deltamod-tauri-shell.exe'),
    path.join(targetDir, 'release', 'deltamod-tauri-shell.exe')
];
const executable = candidates.find(candidate => fs.existsSync(candidate));
if (!executable) {
    throw new Error(`Tauri did not produce the installer shell. Looked in: ${candidates.join(', ')}`);
}

const destination = path.join(root, 'dist', 'Deltamod Community Setup.exe');
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.copyFileSync(executable, destination);
console.log(`Branded setup written to ${path.relative(root, destination)}`);
