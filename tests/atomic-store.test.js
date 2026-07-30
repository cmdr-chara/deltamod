// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { readJsonSync, writeJsonAtomicSync } = require('../node/storage/AtomicStore');

const temporaryDirectories = [];

afterEach(() => {
    while (temporaryDirectories.length) {
        fs.rmSync(temporaryDirectories.pop(), { recursive: true, force: true });
    }
});

describe('AtomicStore', () => {
    it('writes valid JSON and preserves a backup', () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-store-test-'));
        temporaryDirectories.push(root);
        const file = path.join(root, 'store.json');

        writeJsonAtomicSync(file, { version: 1 });
        writeJsonAtomicSync(file, { version: 2 });

        expect(readJsonSync(file)).toEqual({ version: 2 });
        expect(readJsonSync(`${file}.backup`)).toEqual({ version: 1 });
    });
});
