// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
const { planCommand, exitCode } = require('../scripts/tauri-command');

describe('native build command', () => {
    it.each([
        ['win32', 'x64', 'x86_64-pc-windows-msvc', 'nsis'],
        ['linux', 'x64', 'x86_64-unknown-linux-gnu', 'deb'],
        ['darwin', 'x64', 'x86_64-apple-darwin', 'app,dmg'],
        ['darwin', 'arm64', 'aarch64-apple-darwin', 'app,dmg']
    ])('selects %s %s target and package', (os, arch, target, bundles) => {
        expect(planCommand(['build'], {}, os, arch)).toEqual(['build', '--target', target, '--bundles', bundles]);
        expect(planCommand(['dev'], {}, os, arch)).toEqual(['dev', '--target', target]);
        expect(planCommand(['build', '--no-bundle'], {}, os, arch)).not.toContain('--bundles');
    });
    it('keeps explicit bundle choices and an Intel target on an ARM Mac', () => {
        expect(planCommand(['build', '--bundles', 'app'], { TAURI_BUILD_TARGET: 'x86_64-apple-darwin' }, 'darwin', 'arm64'))
            .toEqual(['build', '--target', 'x86_64-apple-darwin', '--bundles', 'app']);
    });
    it.each(['--target', '--target=other', '-t', '-tother'])('rejects staging target drift via %s', option => {
        expect(() => planCommand(['build', option], {}, 'linux', 'x64')).toThrow('TAURI_BUILD_TARGET');
    });
    it('rejects unsupported hosts and cross-OS packaging', () => {
        expect(() => planCommand(['build'], {}, 'linux', 'arm64')).toThrow();
        expect(() => planCommand(['build'], { RUST_TARGET: 'x86_64-pc-windows-msvc' }, 'linux', 'x64')).toThrow();
    });
    it('never reports a signalled or failed spawn as success', () => {
        expect(exitCode({ status: null, signal: 'SIGTERM' })).toBe(1);
        expect(exitCode({ status: null, error: new Error('spawn') })).toBe(1);
        expect(exitCode({ status: 17 })).toBe(17);
        expect(exitCode({ status: 0 })).toBe(0);
    });
});


describe('platform resource boundaries', () => {
    const fs = require('node:fs');
    const path = require('node:path');
    const root = path.resolve(__dirname, '..');
    const read = name => JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', name), 'utf8'));
    it('keeps the Windows controller executable out of Linux/macOS bundles', () => {
        const base = read('tauri.conf.json');
        expect(base.bundle.resources['../tools/cmodeutil.exe']).toBeUndefined();
        expect(read('tauri.windows.conf.json').bundle.resources['../tools/cmodeutil.exe']).toBe('tools/cmodeutil.exe');
        expect(read('tauri.linux.conf.json').bundle.targets).toEqual(['deb']);
        expect(read('tauri.macos.conf.json').bundle.targets).toEqual(['app', 'dmg']);
        expect(read('tauri.windows.conf.json').bundle.createUpdaterArtifacts).toBe(true);
        expect(read('tauri.macos.conf.json').bundle.createUpdaterArtifacts).toBe(true);
        expect(base.bundle.createUpdaterArtifacts).toBe(false);
    });
    it('has no active Electron runtime or dependency binding', () => {
        const { verify } = require('../scripts/verify-tauri-only');
        expect(verify(root)).toEqual([]);
    });
});
