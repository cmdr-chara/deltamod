// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const {
    TARGETS,
    sha256File,
    targetForCurrentPlatform,
    treeDigest,
    verifyInstallation,
    validateProvenance
} = require('../scripts/lib/undertale-mod-tool-provenance');
const { undertaleModCliPathFor } = require('../node/GamePatching');

const root = path.resolve(__dirname, '..');
const manifest = JSON.parse(fs.readFileSync(
    path.join(root, 'tools', 'UndertaleModTool.provenance.json'),
    'utf8'
));

describe('UndertaleModTool provenance', () => {
    it('pins the approved source release and CLI artifacts', () => {
        expect(validateProvenance(structuredClone(manifest))).toEqual(manifest);
        expect(Object.keys(TARGETS)).toEqual(['win32-x64', 'linux-x64', 'darwin-x64']);
    });

    it('maps supported platforms and rejects unavailable architectures', () => {
        expect(undertaleModCliPathFor('win32', 'x64')).toMatch(/[\\/]win-x64[\\/]UndertaleModCli\.exe$/);
        expect(undertaleModCliPathFor('linux', 'x64')).toMatch(/[\\/]linux-x64[\\/]UndertaleModCli$/);
        expect(undertaleModCliPathFor('darwin', 'x64')).toMatch(/[\\/]mac-x64[\\/]UndertaleModCli$/);
        expect(() => undertaleModCliPathFor('darwin', 'arm64')).toThrow(/not packaged/i);
        if (TARGETS[`${process.platform}-${process.arch}`]) {
            expect(targetForCurrentPlatform()).toBe(`${process.platform}-${process.arch}`);
        }
    });

    it('rejects altered sources and incomplete checksums', () => {
        const unsafe = structuredClone(manifest);
        unsafe.sourceUrl = 'https://example.com/UndertaleModTool';
        expect(() => validateProvenance(unsafe)).toThrow(/source repository/i);

        const incomplete = structuredClone(manifest);
        incomplete.artifacts['win32-x64'].archiveSha256 = 'abc';
        expect(() => validateProvenance(incomplete)).toThrow(/checksums/i);

        const mismatchedRelease = structuredClone(manifest);
        mismatchedRelease.releaseRevision = '0'.repeat(40);
        expect(() => validateProvenance(mismatchedRelease)).toThrow(/source revision/i);
    });

    it('detects changed, missing, and unexpected runtime files', () => {
        const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-utmt-tree-test-'));
        try {
            const directory = path.join(temporary, 'tools', 'undertale-mod-tool', 'win-x64');
            fs.mkdirSync(directory, { recursive: true });
            fs.writeFileSync(path.join(directory, 'UndertaleModCli.exe'), 'executable');
            fs.writeFileSync(path.join(directory, 'LICENSE.txt'), 'license');
            fs.writeFileSync(path.join(directory, 'runtime.dll'), 'runtime');
            const provenance = structuredClone(manifest);
            const tree = treeDigest(directory);
            provenance.artifacts['win32-x64'].executableSha256 = sha256File(path.join(directory, 'UndertaleModCli.exe'));
            provenance.artifacts['win32-x64'].treeFileCount = tree.fileCount;
            provenance.artifacts['win32-x64'].treeSha256 = tree.sha256;

            expect(verifyInstallation(temporary, provenance, 'win32-x64')).toBe(path.join(directory, 'UndertaleModCli.exe'));
            fs.writeFileSync(path.join(directory, 'runtime.dll'), 'changed');
            expect(() => verifyInstallation(temporary, provenance, 'win32-x64')).toThrow(/tree verification/i);
            fs.writeFileSync(path.join(directory, 'runtime.dll'), 'runtime');
            fs.writeFileSync(path.join(directory, 'unexpected.dll'), 'unexpected');
            expect(() => verifyInstallation(temporary, provenance, 'win32-x64')).toThrow(/tree verification/i);
            fs.rmSync(path.join(directory, 'unexpected.dll'));
            fs.rmSync(path.join(directory, 'runtime.dll'));
            expect(() => verifyInstallation(temporary, provenance, 'win32-x64')).toThrow(/tree verification/i);
        } finally {
            fs.rmSync(temporary, { recursive: true, force: true });
        }
    });
});
