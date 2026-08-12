// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { validateExtractedTreeFallback } = require('../node/security/ArchiveSecurity');
const { validateExtractedTreeNative, _protocol } = require('../node/security/NativeArchiveSecurity');

const roots = [];
afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-tree-validation-'));
    roots.push(root);
    return root;
}

function debugBinary() {
    const executable = process.platform === 'win32' ? 'deltamod-security-worker.exe' : 'deltamod-security-worker';
    return path.join(__dirname, '..', 'native', 'target', 'debug', executable);
}

const limits = {
    maxFiles: 10,
    maxExpandedBytes: 1024,
    maxArchiveBytes: 1024,
    maxDepth: 4
};

describe('extracted-tree validation', () => {
    it('retains the named JavaScript fallback result and limits', async () => {
        const root = fixture();
        fs.mkdirSync(path.join(root, 'nested'));
        fs.writeFileSync(path.join(root, 'one'), '123');
        fs.writeFileSync(path.join(root, 'nested', 'two'), '4567');
        await expect(validateExtractedTreeFallback(root, limits)).resolves.toEqual({
            fileCount: 2,
            expandedBytes: 7
        });
        await expect(validateExtractedTreeFallback(root, { ...limits, maxFiles: 1 }))
            .rejects.toMatchObject({ code: 'ARCHIVE_FILE_LIMIT' });
        await expect(validateExtractedTreeFallback(root, { ...limits, maxExpandedBytes: 6 }))
            .rejects.toMatchObject({ code: 'ARCHIVE_SIZE_LIMIT' });
    });

    it('rejects hardlinked regular files in the JavaScript fallback', async () => {
        const root = fixture();
        const source = path.join(root, 'source');
        fs.writeFileSync(source, 'data');
        fs.linkSync(source, path.join(root, 'second'));
        await expect(validateExtractedTreeFallback(root, limits))
            .rejects.toMatchObject({ code: 'ARCHIVE_LINK_BLOCKED' });
    });

    it('strictly validates bounded native responses', () => {
        expect(_protocol.parseResponse('{"ok":true,"fileCount":1,"expandedBytes":2}\n'))
            .toEqual({ fileCount: 1, expandedBytes: 2 });
        expect(() => _protocol.parseResponse('{"ok":true,"fileCount":1,"expandedBytes":2,"path":"untrusted"}\n'))
            .toThrow(/invalid success response/i);
        expect(() => _protocol.parseResponse('{}\n')).toThrow(/schema/i);
        expect(() => _protocol.parseResponse('{}\n{}\n')).toThrow(/one JSON/i);
    });

    it('runs the actual debug native validator', async () => {
        const binary = debugBinary();
        expect(fs.existsSync(binary)).toBe(true);
        const root = fixture();
        fs.mkdirSync(path.join(root, 'nested'));
        fs.writeFileSync(path.join(root, 'nested', 'file'), 'native');
        await expect(validateExtractedTreeNative(root, limits, { sidecarPath: binary }))
            .resolves.toEqual({ fileCount: 1, expandedBytes: 6 });
        await expect(validateExtractedTreeNative(root, { ...limits, maxExpandedBytes: 5 }, { sidecarPath: binary }))
            .rejects.toMatchObject({ code: 'ARCHIVE_SIZE_LIMIT' });
    });

    it('returns null only for an unavailable binary and fails closed otherwise', async () => {
        const root = fixture();
        expect(validateExtractedTreeNative(root, limits, { sidecarPath: path.join(root, 'missing') })).toBeNull();
        await expect(validateExtractedTreeNative(root, limits, { sidecarPath: process.execPath }))
            .rejects.toMatchObject({ code: 'ARCHIVE_NATIVE_FAILED' });
    });
});
