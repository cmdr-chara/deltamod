// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const {
    TARGETS,
    targetForCurrentPlatform,
    validateProvenance
} = require('../scripts/lib/g3mtool-provenance');
const { patcherPathFor } = require('../node/GamePatching');
const {
    assertDownloadUrl,
    validateArchiveEntries
} = require('../scripts/acquire-g3mtool');

const root = path.resolve(__dirname, '..');
const packageJson = require('../package.json');
const manifest = JSON.parse(fs.readFileSync(
    path.join(root, 'tools', 'G3MTool.provenance.json'),
    'utf8'
));

describe('G3MTool provenance', () => {
    it('pins the approved source, release, artifact sizes, and checksums', () => {
        expect(validateProvenance(structuredClone(manifest))).toEqual(manifest);
        expect(Object.keys(TARGETS)).toEqual([
            'win32-x64',
            'linux-x64',
            'darwin-x64',
            'darwin-arm64'
        ]);
    });

    it('maps both macOS architectures to their native patchers', () => {
        expect(patcherPathFor('darwin', 'x64')).toMatch(/[\\/]mac-x64[\\/]G3MTool$/);
        expect(patcherPathFor('darwin', 'arm64')).toMatch(/[\\/]mac-arm64[\\/]G3MTool$/);
        expect(() => patcherPathFor('darwin', 'ia32')).toThrow(/not packaged/i);
        expect(targetForCurrentPlatform()).toBe(`${process.platform}-${process.arch}`);
    });

    it('packages unsigned macOS DMG and ZIP artifacts for Intel and Apple Silicon', () => {
        expect(packageJson.scripts['build-macos']).toContain('electron-builder --mac');
        expect(packageJson.build.mac.category).toBe('public.app-category.utilities');
        expect(packageJson.build.mac.icon).toBe('build/icon-macos.png');
        expect(packageJson.build.mac.target).toEqual([
            { target: 'dmg', arch: ['x64', 'arm64'] },
            { target: 'zip', arch: ['x64', 'arm64'] }
        ]);
    });

    it('rejects an unapproved source or incomplete checksum', () => {
        const unsafe = structuredClone(manifest);
        unsafe.sourceUrl = 'https://example.com/G3MTool';
        expect(() => validateProvenance(unsafe)).toThrow(/approved upstream/i);

        const incomplete = structuredClone(manifest);
        incomplete.artifacts['win32-x64'].archiveSha256 = 'abc';
        expect(() => validateProvenance(incomplete)).toThrow(/SHA-256/i);
    });

    it('allows only approved HTTPS download hosts', () => {
        expect(assertDownloadUrl(manifest.artifacts['win32-x64'].archiveUrl).hostname).toBe('github.com');
        expect(() => assertDownloadUrl('http://github.com/file.zip')).toThrow(/unapproved/i);
        expect(() => assertDownloadUrl('https://example.com/file.zip')).toThrow(/unapproved/i);
    });

    it('rejects traversal and unexpected archive entries', () => {
        const artifact = manifest.artifacts['win32-x64'];
        expect(() => validateArchiveEntries([
            { name: 'G3MTool.exe', attr: '....A' },
            { name: 'licenses/GPL-3.0.txt', attr: '....A' },
            { name: 'GameSpecificData/Definitions/deltarune.json', attr: '....A' }
        ], artifact)).not.toThrow();
        expect(() => validateArchiveEntries([
            { name: '../G3MTool.exe', attr: '....A' }
        ], artifact)).toThrow(/unsafe/i);
        expect(() => validateArchiveEntries([
            { name: 'unexpected.dll', attr: '....A' }
        ], artifact)).toThrow(/unexpected/i);
    });
});
