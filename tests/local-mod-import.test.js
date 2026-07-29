// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { stageLocalArchive, validateLocalArchive } = require('../node/protocol/LocalModImport');

describe('Local Community mod imports', () => {
    let root;

    beforeEach(() => {
        root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-local-import-'));
    });

    afterEach(() => {
        fs.rmSync(root, { recursive: true, force: true });
    });

    it('stages a regular archive without changing the source', async () => {
        const source = path.join(root, 'source.modarchive');
        const staging = path.join(root, 'staging');
        fs.writeFileSync(source, Buffer.from('PK\u0003\u0004archive'));
        const staged = await stageLocalArchive(source, staging);
        expect(staged).not.toBe(source);
        expect(fs.readFileSync(staged)).toEqual(fs.readFileSync(source));
        expect(fs.existsSync(source)).toBe(true);
    });

    it('rejects unsupported extensions and relative paths', async () => {
        const source = path.join(root, 'source.exe');
        fs.writeFileSync(source, 'not an archive');
        await expect(validateLocalArchive(source)).rejects.toThrow('Only .modarchive and .zip');
        await expect(validateLocalArchive('relative.modarchive')).rejects.toThrow('must be absolute');
    });

    if (process.platform === 'win32') {
        it('rejects UNC and Windows device paths', async () => {
            await expect(validateLocalArchive('\\\\server\\share\\mod.modarchive')).rejects.toThrow(
                'UNC and Windows device paths'
            );
            await expect(validateLocalArchive('\\\\?\\C:\\temp\\mod.modarchive')).rejects.toThrow(
                'UNC and Windows device paths'
            );
        });
    }

    it('rejects hardlinked archives', async () => {
        const source = path.join(root, 'source.modarchive');
        const linked = path.join(root, 'linked.modarchive');
        fs.writeFileSync(source, 'archive');
        fs.linkSync(source, linked);
        await expect(validateLocalArchive(source)).rejects.toThrow('regular, non-linked file');
    });

    it('rejects archives reached through a linked directory', async () => {
        const realDirectory = path.join(root, 'real');
        const linkedDirectory = path.join(root, 'linked');
        fs.mkdirSync(realDirectory);
        fs.writeFileSync(path.join(realDirectory, 'source.modarchive'), 'archive');
        fs.symlinkSync(realDirectory, linkedDirectory, process.platform === 'win32' ? 'junction' : 'dir');
        await expect(validateLocalArchive(path.join(linkedDirectory, 'source.modarchive'))).rejects.toThrow(
            /linked or invalid parent|linked path/
        );
    });
});
