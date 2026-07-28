const fs = require('fs');
const os = require('os');
const path = require('path');
const { afterEach, describe, expect, it } = globalThis;
const { buildPatchPlan, restore } = require('../node/GamePatching');

const roots = [];

function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-patch-test-'));
    roots.push(root);
    const game = path.join(root, 'game');
    const mods = path.join(root, 'mods');
    const mod = path.join(mods, 'Mod_example');
    fs.mkdirSync(path.join(game, 'data'), { recursive: true });
    fs.mkdirSync(path.join(mod, 'files'), { recursive: true });
    fs.writeFileSync(path.join(game, 'data', 'target.txt'), 'original');
    fs.writeFileSync(path.join(mod, 'files', 'replacement.txt'), 'replacement');
    fs.writeFileSync(path.join(mod, 'meta.toml'), '[metadata]\nname="Example"');
    fs.writeFileSync(path.join(mod, '__deltaID.json'), '{"uniqueId":"example-id"}');
    return { root, game, mods, mod };
}

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

describe('patch planning', () => {
    it('builds a contained direct patch plan', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="files/replacement.txt" to="data/target.txt"/></root>');
        const plan = buildPatchPlan(data.game, data.mods, ['example-id']);
        expect(plan.operationCount).toBe(1);
        expect(plan.direct[0].target).toBe(path.join(data.game, 'data', 'target.txt'));
    });

    it('rejects a target that escapes the game', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="files/replacement.txt" to="../outside.txt"/></root>');
        expect(() => buildPatchPlan(data.game, data.mods, ['example-id'])).toThrow(/traversal|escapes/i);
    });

    it('rejects a source that escapes the mod', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="../outside.txt" to="data/target.txt"/></root>');
        expect(() => buildPatchPlan(data.game, data.mods, ['example-id'])).toThrow(/traversal|escapes/i);
    });

    it('recovers only files named by the transaction journal', () => {
        const data = fixture();
        const unrelatedBackup = path.join(data.game, 'unrelated.bak');
        const target = path.join(data.game, 'data', 'target.txt');
        const backupRoot = path.join(data.game, '.deltamod-community-patch-backups', '123-456');
        const backup = path.join(backupRoot, 'data', 'target.txt');
        fs.mkdirSync(path.dirname(backup), { recursive: true });
        fs.renameSync(target, backup);
        fs.writeFileSync(target, 'patched');
        fs.writeFileSync(unrelatedBackup, 'belongs to the game');
        fs.writeFileSync(
            path.join(data.game, '.deltamod-community-patch-journal.json'),
            JSON.stringify({
                schemaVersion: 1,
                transactionId: '123-456',
                state: 'patched',
                operations: [{ type: 'restore', target: 'data/target.txt', backup: 'data/target.txt' }]
            })
        );

        restore(data.game);

        expect(fs.readFileSync(target, 'utf8')).toBe('original');
        expect(fs.readFileSync(unrelatedBackup, 'utf8')).toBe('belongs to the game');
        expect(fs.existsSync(path.join(data.game, '.deltamod-community-patch-journal.json'))).toBe(false);
    });

    it('refuses an invalid recovery journal without changing files', () => {
        const data = fixture();
        const target = path.join(data.game, 'data', 'target.txt');
        fs.writeFileSync(
            path.join(data.game, '.deltamod-community-patch-journal.json'),
            JSON.stringify({ schemaVersion: 1, transactionId: '../../escape', operations: [] })
        );

        expect(() => restore(data.game)).toThrow(/journal is invalid/i);
        expect(fs.readFileSync(target, 'utf8')).toBe('original');
    });
});
