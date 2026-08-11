// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { afterEach, describe, expect, it } = globalThis;
const {
    emptyCache,
    loadHashCache,
    hashGameFile,
    saveHashCache
} = require('../node/storage/GameHashCache');

const roots = [];

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-hash-cache-'));
    roots.push(root);
    const game = path.join(root, 'game');
    fs.mkdirSync(game);
    fs.writeFileSync(path.join(game, 'data.win'), 'game data');
    return { root, game, cachePath: path.join(root, 'profile', '_game-hashes.json') };
}

describe('Community game hash cache', () => {
    it('stores hashes in the Community profile instead of beside game files', () => {
        const data = fixture();
        const cache = emptyCache();
        const result = hashGameFile(data.game, 'data.win', cache);
        saveHashCache(data.cachePath, cache);

        expect(result.sha256).toBe(crypto.createHash('sha256').update('game data').digest('hex'));
        expect(fs.existsSync(path.join(data.game, 'data.win.hash'))).toBe(false);
        expect(loadHashCache(data.cachePath).entries['data.win'].sha256).toBe(result.sha256);
    });

    it('rejects traversal outside the game root', () => {
        const data = fixture();
        expect(() => hashGameFile(data.game, '../outside', emptyCache())).toThrow(/traversal|escapes/i);
    });

    it('invalidates a cached hash when the file changes', async () => {
        const data = fixture();
        const cache = emptyCache();
        const first = hashGameFile(data.game, 'data.win', cache);
        await new Promise(resolve => setTimeout(resolve, 10));
        fs.writeFileSync(path.join(data.game, 'data.win'), 'changed game data');
        const second = hashGameFile(data.game, 'data.win', cache);
        expect(first.sha256).not.toBe(second.sha256);
        expect(second.updated).toBe(true);
    });

    it('reuses only a valid digest with a matching safe file signature', () => {
        const data = fixture();
        const cache = emptyCache();
        hashGameFile(data.game, 'data.win', cache);
        cache.entries['data.win'].sha256 = '0'.repeat(64);

        expect(hashGameFile(data.game, 'data.win', cache)).toEqual({
            sha256: '0'.repeat(64),
            updated: false
        });

        cache.entries['data.win'].sha256 = 'malformed';
        const refreshed = hashGameFile(data.game, 'data.win', cache);
        expect(refreshed.updated).toBe(true);
        expect(refreshed.sha256).toMatch(/^[a-f0-9]{64}$/);
    });
});
