// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const {
    UnsafePathError,
    resolveWithin,
    protocolPath,
    validateRelativePath
} = require('../node/security/PathSecurity');

const temporaryDirectories = [];

function makeRoot() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-path-test-'));
    temporaryDirectories.push(root);
    fs.mkdirSync(path.join(root, 'mods'), { recursive: true });
    return root;
}

afterEach(() => {
    while (temporaryDirectories.length) {
        fs.rmSync(temporaryDirectories.pop(), { recursive: true, force: true });
    }
});

describe('resolveWithin', () => {
    it('resolves a normal relative path', () => {
        const root = makeRoot();
        expect(resolveWithin(root, 'mods/example.txt')).toBe(path.join(root, 'mods', 'example.txt'));
    });

    it.each([
        '../outside.txt',
        'mods/../../outside.txt',
        '%2e%2e%2foutside.txt',
        'C:\\Windows\\system.ini',
        '\\rooted',
        '\\\\server\\share\\file',
        '//server/share/file'
    ])('rejects unsafe path %s', candidate => {
        const root = makeRoot();
        expect(() => resolveWithin(root, candidate)).toThrow(UnsafePathError);
    });

    it('rejects a symlink that escapes the root when supported', () => {
        const root = makeRoot();
        const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-outside-test-'));
        temporaryDirectories.push(outside);
        const link = path.join(root, 'mods', 'escape');

        try {
            fs.symlinkSync(outside, link, process.platform === 'win32' ? 'junction' : 'dir');
        } catch {
            return;
        }

        expect(() => resolveWithin(root, 'mods/escape/file.txt')).toThrow(UnsafePathError);
    });
});

describe('protocolPath', () => {
    it('maps a safe protocol URL to a relative path', () => {
        expect(protocolPath('deltapack://web/index.html')).toBe(path.join('web', 'index.html'));
    });

    it('rejects encoded traversal in a protocol URL', () => {
        expect(() => protocolPath('deltapack://web/%252e%252e/secret.txt')).toThrow(UnsafePathError);
    });
});

describe('validateRelativePath', () => {
    it('rejects null bytes', () => {
        expect(() => validateRelativePath('file\0.txt')).toThrow(UnsafePathError);
    });
});
