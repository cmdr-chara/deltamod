// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { resolveTauriTarget } = require('./lib/tauri-target');

/** Use the same target as resource staging, including on Windows without a shell. */
function planCommand(args, env = process.env, platform = process.platform, arch = process.arch) {
    const [command, ...options] = args;
    if (!['dev', 'build'].includes(command)) throw new Error('Expected dev or build.');
    // Staging runs before this command. A different CLI target would bundle the
    // wrong workers/tools. Set TAURI_BUILD_TARGET before the entire npm command.
    if (options.some(arg => arg === '--target' || arg === '-t' || arg.startsWith('--target=') || /^-t.+/.test(arg))) {
        throw new Error('Set TAURI_BUILD_TARGET before npm so staging and the app use the same target.');
    }
    const target = resolveTauriTarget(undefined, env, platform, arch);
    const targetPlatform = target.includes('windows') ? 'win32' : target.includes('darwin') ? 'darwin' : 'linux';
    if (targetPlatform !== platform) throw new Error('Build on the target operating system.');
    const result = [command, '--target', target, ...options];
    const explicitBundles = options.some(arg => arg === '--bundles' || arg === '-b' || arg.startsWith('--bundles=') || /^-b.+/.test(arg));
    if (command === 'build' && !options.includes('--no-bundle') && !explicitBundles) {
        result.push('--bundles', { win32: 'nsis', linux: 'deb', darwin: 'app,dmg' }[platform]);
    }
    return result;
}

function exitCode(result) {
    return !result.error && !result.signal && Number.isInteger(result.status) ? result.status : 1;
}

function run(args) {
    const planned = planCommand(args);
    const manifest = require.resolve('@tauri-apps/cli/package.json');
    const cli = path.resolve(path.dirname(manifest), require(manifest).bin.tauri);
    const result = spawnSync(process.execPath, [cli, ...planned], {
        cwd: path.resolve(__dirname, '..'), stdio: 'inherit', shell: false
    });
    if (result.error) console.error(result.error.message);
    return exitCode(result);
}

if (require.main === module) {
    try { process.exitCode = run(process.argv.slice(2)); }
    catch (error) { console.error(error.message); process.exitCode = 1; }
}
module.exports = { planCommand, exitCode };
