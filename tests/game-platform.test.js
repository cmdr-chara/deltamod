// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-30.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { afterEach, describe, expect, it } = globalThis;
const {
    createLaunchSpec,
    mapPatchTarget,
    resolveGameInstallation
} = require('../node/GamePlatform');

const roots = [];

function fixture(files) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-platform-test-'));
    roots.push(root);
    for (const file of files) {
        const target = path.join(root, file);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, '');
    }
    return root;
}

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

describe('native game platform resolution', () => {
    const undertale = {
        exeName: 'UNDERTALE.exe',
        platforms: {
            linux: {
                executable: 'run.sh',
                dataFiles: ['assets/game.unx'],
                contentRoot: 'assets',
                patchLayout: 'gamemaker-linux-assets'
            },
            darwin: {
                bundle: 'UNDERTALE.app',
                executable: 'UNDERTALE.app/Contents/MacOS/Mac_Runner',
                dataFiles: ['UNDERTALE.app/Contents/Resources/game.ios'],
                contentRoot: 'UNDERTALE.app/Contents/Resources',
                patchLayout: 'gamemaker-mac-resources'
            }
        }
    };

    it('detects and launches the native Linux layout', () => {
        const root = fixture(['run.sh', 'assets/game.unx']);
        const resolution = resolveGameInstallation(undertale, root, { hostPlatform: 'linux' });
        expect(resolution.platform).toBe('linux');
        expect(resolution.mapPatchTarget('data.win')).toBe('assets/game.unx');
        expect(resolution.mapPatchTarget('mus/track.ogg')).toBe('assets/mus/track.ogg');
        const launch = createLaunchSpec(resolution);
        expect(launch.command).toBe('sh');
        expect(launch.args).toEqual([path.join(root, 'run.sh')]);
    });

    it('detects and launches a native macOS app bundle', () => {
        const root = fixture([
            'UNDERTALE.app/Contents/MacOS/Mac_Runner',
            'UNDERTALE.app/Contents/Resources/game.ios'
        ]);
        const resolution = resolveGameInstallation(undertale, root, { hostPlatform: 'darwin' });
        expect(resolution.platform).toBe('darwin');
        expect(resolution.mapPatchTarget('data.win')).toBe('UNDERTALE.app/Contents/Resources/game.ios');
        expect(resolution.mapPatchTarget('mus/track.ogg')).toBe('UNDERTALE.app/Contents/Resources/mus/track.ogg');
        const launch = createLaunchSpec(resolution);
        expect(launch.command).toBe('open');
        expect(launch.args).toEqual(['-W', path.join(root, 'UNDERTALE.app')]);
    });

    it('falls back to a Windows installation through Wine on Linux', () => {
        const root = fixture(['UNDERTALE.exe', 'data.win']);
        const resolution = resolveGameInstallation(undertale, root, { hostPlatform: 'linux' });
        expect(resolution.platform).toBe('win32');
    });

    it('does not run a stored Windows installation as a native macOS game', () => {
        const root = fixture(['UNDERTALE.exe', 'data.win']);
        const resolution = resolveGameInstallation(undertale, root, {
            hostPlatform: 'darwin',
            preferredPlatform: 'win32'
        });
        expect(resolution).toBeNull();
    });

    it('maps DELTARUNE chapter data into the macOS bundle', () => {
        const game = require('../games/toby.deltarune.json');
        const root = fixture([
            'DELTARUNE.app/Contents/MacOS/Mac_Runner',
            'DELTARUNE.app/Contents/Resources/game.ios'
        ]);
        const resolution = resolveGameInstallation(game, root, { hostPlatform: 'darwin' });
        expect(resolution.platform).toBe('darwin');
        expect(mapPatchTarget('chapter4_windows/data.win', resolution.definition))
            .toBe('DELTARUNE.app/Contents/Resources/chapter4_mac/game.ios');
        expect(mapPatchTarget('chapter4_windows/lang/en.json', resolution.definition))
            .toBe('DELTARUNE.app/Contents/Resources/chapter4_mac/lang/en.json');
    });

    it('does not normalize traversal into a platform content directory', () => {
        expect(() => mapPatchTarget('../outside.txt', {
            contentRoot: 'assets',
            patchLayout: 'gamemaker-linux-assets'
        })).toThrow(/unsafe platform path/i);
    });
});
