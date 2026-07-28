const fs = require('fs');
const path = require('path');
const {
    validateProvenance
} = require('../scripts/lib/g3mtool-provenance');
const {
    assertDownloadUrl,
    validateArchiveEntries
} = require('../scripts/acquire-g3mtool');

const root = path.resolve(__dirname, '..');
const manifest = JSON.parse(fs.readFileSync(
    path.join(root, 'tools', 'G3MTool.provenance.json'),
    'utf8'
));

describe('G3MTool provenance', () => {
    it('pins the approved source, release, artifact sizes, and checksums', () => {
        expect(validateProvenance(structuredClone(manifest))).toEqual(manifest);
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
