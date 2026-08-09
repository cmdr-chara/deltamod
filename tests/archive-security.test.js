// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { extractArchiveAtomic, validateArchiveEntries } = require('../node/security/ArchiveSecurity');

const temporaryDirectories = [];

afterEach(() => {
    while (temporaryDirectories.length) fs.rmSync(temporaryDirectories.pop(), { recursive: true, force: true });
});

describe('archive validation', () => {
    it('accepts a bounded ordinary archive inventory', () => {
        expect(validateArchiveEntries([
            { name: 'mod/meta.toml', size: '100', attr: 'A' },
            { name: 'mod/data/file.bin', size: '900', attr: 'A' }
        ])).toEqual({ fileCount: 2, expandedBytes: 1000 });
    });

    it.each([
        '../outside.txt',
        'C:\\outside.txt',
        '\\\\server\\share\\outside.txt',
        '/absolute.txt'
    ])('rejects escaping entry %s', name => {
        expect(() => validateArchiveEntries([{ name, size: '1', attr: 'A' }]))
            .toThrow(/Unsafe archive entry/);
    });

    it('rejects duplicate destinations and links', () => {
        expect(() => validateArchiveEntries([
            { name: 'same.txt', size: '1', attr: 'A' },
            { name: 'same.txt', size: '1', attr: 'A' }
        ])).toThrow(/duplicate/i);
        expect(() => validateArchiveEntries([
            { name: 'linked', size: '1', attr: 'lrwxrwxrwx' }
        ])).toThrow(/links/i);
    });

    it('enforces expanded-size and entry limits', () => {
        expect(() => validateArchiveEntries(
            [{ name: 'large.bin', size: '101', attr: 'A' }],
            { maxExpandedBytes: 100 }
        )).toThrow(/expands beyond/i);
        expect(() => validateArchiveEntries(
            [
                { name: 'one', size: '1', attr: 'A' },
                { name: 'two', size: '1', attr: 'A' }
            ],
            { maxFiles: 1 }
        )).toThrow(/contains 2 entries/i);
    });

    it('rejects non-integer sizes and invalid limits', () => {
        expect(() => validateArchiveEntries([
            { name: 'fractional.bin', size: '1.5', attr: 'A' }
        ])).toThrow(/Invalid archive entry size/i);
        expect(() => validateArchiveEntries([
            { name: 'unsafe-integer.bin', size: String(Number.MAX_SAFE_INTEGER + 1), attr: 'A' }
        ])).toThrow(/Invalid archive entry size/i);
        expect(() => validateArchiveEntries([
            { name: 'file.bin', size: '1', attr: 'A' }
        ], { maxFiles: -1 })).toThrow(/Invalid archive limit/i);
    });

    it('rejects linked archive inputs before invoking the extractor', async () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-archive-input-'));
        temporaryDirectories.push(root);
        const archive = path.join(root, 'source.zip');
        fs.writeFileSync(archive, 'not needed');
        const hardlink = path.join(root, 'hardlink.zip');
        fs.linkSync(archive, hardlink);
        await expect(extractArchiveAtomic(hardlink, path.join(root, 'destination')))
            .rejects.toMatchObject({ code: 'ARCHIVE_LINK_BLOCKED' });

        fs.unlinkSync(hardlink);
        const symlink = path.join(root, 'symlink.zip');
        try {
            fs.symlinkSync(archive, symlink, 'file');
        } catch {
            return;
        }
        await expect(extractArchiveAtomic(symlink, path.join(root, 'destination')))
            .rejects.toMatchObject({ code: 'ARCHIVE_LINK_BLOCKED' });
    });
});
