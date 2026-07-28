const { validateArchiveEntries } = require('../node/security/ArchiveSecurity');

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
});
