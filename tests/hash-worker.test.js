// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { Worker } = require('worker_threads');
const { afterEach, describe, expect, it } = globalThis;
const { _protocol } = require('../node/workers/NativeHashWorker');

const roots = [];
afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

describe('directory hash worker', () => {
    it('rejects malformed native file events before they reach the cache', () => {
        const valid = {
            relative: 'lang/en.json',
            signature: '12:1234.5',
            sha256: 'a'.repeat(64),
            completed: 1,
            total: 1
        };
        expect(_protocol.validateFileEvent(valid, null, new Set())).toEqual({
            relative: 'lang/en.json',
            total: 1
        });
        expect(() => _protocol.validateFileEvent({ ...valid, relative: '../outside' }, null, new Set())).toThrow(/unsafe relative path/i);
        expect(() => _protocol.validateFileEvent({ ...valid, sha256: 'invalid' }, null, new Set())).toThrow(/SHA-256/i);
        expect(() => _protocol.validateFileEvent({ ...valid, signature: '12:' }, null, new Set())).toThrow(/signature/i);
        expect(() => _protocol.validateFileEvent({ ...valid, completed: 2 }, null, new Set())).toThrow(/progress/i);
    });

    it('preserves progress and cache behavior when the native binary is unavailable', async () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-hash-worker-'));
        roots.push(root);
        const game = path.join(root, 'game');
        fs.mkdirSync(game);
        fs.writeFileSync(path.join(game, 'data.win'), 'game data');
        const cachePath = path.join(root, 'profile', '_game-hashes.json');
        const messages = [];
        const worker = new Worker(path.join(__dirname, '..', 'node', 'workers', 'HashWorker.js'), {
            workerData: {
                root: game,
                cachePath,
                operationId: 'operation-1',
                sidecarPath: path.join(root, 'missing.exe')
            }
        });
        await new Promise((resolve, reject) => {
            worker.on('message', message => {
                messages.push(message);
                if (message.done) resolve();
                if (message.error) reject(new Error(message.error.message));
            });
            worker.on('error', reject);
        });

        expect(messages[0]).toEqual({
            operationId: 'operation-1',
            phase: 'hashing',
            completed: 1,
            total: 1,
            currentItem: 'data.win'
        });
        expect(messages[1]).toEqual({ done: true, fileCount: 1 });
        expect(JSON.parse(fs.readFileSync(cachePath, 'utf8'))).toMatchObject({
            schemaVersion: 1,
            entries: { 'data.win': { sha256: expect.stringMatching(/^[a-f0-9]{64}$/) } }
        });
    });
});
