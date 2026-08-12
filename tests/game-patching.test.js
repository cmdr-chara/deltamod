// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { EventEmitter } = require('events');
const { afterEach, describe, expect, it } = globalThis;
const {
    assertCsxPlatformSupported,
    buildPatchPlan,
    restore,
    startGamePatch,
    terminateProcessTree
} = require('../node/GamePatching');

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
    it('preflights CSX platform support', () => {
        expect(() => assertCsxPlatformSupported({ scripts: [{}] }, 'win32', 'x64')).not.toThrow();
        expect(() => assertCsxPlatformSupported({ scripts: [{}] }, 'linux', 'x64')).not.toThrow();
        expect(() => assertCsxPlatformSupported({ scripts: [{}] }, 'darwin', 'x64')).not.toThrow();
        expect(() => assertCsxPlatformSupported({ scripts: [{}] }, 'darwin', 'arm64')).toThrow(/not packaged/i);
        expect(() => assertCsxPlatformSupported({ scripts: [] }, 'darwin', 'arm64')).not.toThrow();
    });

    it('falls back to force-killing an owned child when no process group is available', () => {
        const child = { exitCode: null, kill: vi.fn() };
        terminateProcessTree(child, 'linux');
        expect(child.kill).toHaveBeenCalledWith('SIGKILL');
    });

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

    it('applies a platform target mapping before containment checks', () => {
        const data = fixture();
        fs.mkdirSync(path.join(data.game, 'assets'), { recursive: true });
        fs.writeFileSync(path.join(data.game, 'assets', 'game.unx'), 'original');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="files/replacement.txt" to="data.win"/></root>');
        const plan = buildPatchPlan(data.game, data.mods, ['example-id'], {
            mapPatchTarget: target => target === 'data.win' ? 'assets/game.unx' : target
        });
        expect(plan.direct[0].target).toBe(path.join(data.game, 'assets', 'game.unx'));
    });

    it('preserves patch type order and groups merge operations', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.game, 'data', 'second.txt'), 'original');
        fs.writeFileSync(path.join(data.mod, 'files', 'second.bin'), 'patch');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), [
            '<root>',
            '<patch type="override" patch="files/replacement.txt" to="new.txt"/>',
            '<patch type="xdelta" patch="files/replacement.txt" to="data/target.txt"/>',
            '<patch type="g3mpatch" patch="files/second.bin" to="data/target.txt"/>',
            '<patch type="copy" patch="files/second.bin" to="second-new.txt"/>',
            '</root>'
        ].join(''));
        const plan = buildPatchPlan(data.game, data.mods, ['example-id']);
        expect(plan.direct.map(patch => patch.type)).toEqual(['override', 'copy']);
        expect(plan.merged).toHaveLength(1);
        expect(plan.merged[0].patches.map(patch => patch.type)).toEqual(['xdelta', 'g3mpatch']);
        expect(plan.operationCount).toBe(3);
    });

    it('rejects conflicts between direct, merge, and CSX patch groups', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'script');
        fs.writeFileSync(path.join(data.game, 'data.win'), 'game');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="files/replacement.txt" to="data/target.txt"/><patch type="xdelta" patch="files/replacement.txt" to="data/target.txt"/></root>');
        expect(() => buildPatchPlan(data.game, data.mods, ['example-id'])).toThrow(/direct and merge/i);
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/><patch type="override" patch="files/replacement.txt" to="data.win"/></root>');
        expect(() => buildPatchPlan(data.game, data.mods, ['example-id'])).toThrow(/CSX|non-direct/i);
    });

    it('plans CSX scripts for a GameMaker data file', () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        const script = path.join(data.mod, 'files', 'patch.csx');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(script, 'Data.GeneralInfo.Name.Content = "Patched";');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');

        const plan = buildPatchPlan(data.game, data.mods, ['example-id']);

        expect(plan.scripts).toHaveLength(1);
        expect(plan.scripts[0].patches[0].source).toBe(script);
        expect(plan.scripts[0].target).toBe(gameData);
    });

    it('rejects CSX scripts targeting a non-GameMaker file', () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'return;');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data/target.txt"/></root>');
        expect(() => buildPatchPlan(data.game, data.mods, ['example-id'])).toThrow(/GameMaker data file/i);
    });

    it('stages successful CSX output and keeps it recoverable', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'script');
        fs.writeFileSync(path.join(data.mod, 'files', 'resource.txt'), 'companion');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');
        let invocation;
        const spawnImpl = (command, args, options) => {
            invocation = { command, args, options };
            invocation.scriptContent = fs.readFileSync(args.at(-1), 'utf8');
            invocation.companionContent = fs.readFileSync(path.join(path.dirname(args.at(-1)), 'resource.txt'), 'utf8');
            const child = new EventEmitter();
            child.stdout = new EventEmitter();
            child.stderr = new EventEmitter();
            child.kill = vi.fn();
            queueMicrotask(() => {
                const input = args[1];
                const output = args[args.indexOf('--output') + 1];
                fs.writeFileSync(output, `${fs.readFileSync(input, 'utf8')}-patched`);
                child.emit('exit', 0);
            });
            return child;
        };

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            platform: 'win32',
            arch: 'x64',
            undertaleModCliPath: process.execPath,
            spawnImpl
        });

        expect(result.patched).toBe(true);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original-patched');
        expect(invocation.command).toBe(process.execPath);
        expect(invocation.args.slice(0, 2)).toEqual(['load', expect.any(String)]);
        expect(invocation.args).toContain('--scripts');
        expect(invocation.args.at(-1)).toMatch(/[\\/]files[\\/]patch\.csx$/);
        expect(invocation.scriptContent).toBe('script');
        expect(invocation.companionContent).toBe('companion');
        expect(invocation.options.shell).toBe(false);
        expect(invocation.options.cwd).toBe(path.dirname(process.execPath));
        expect(invocation.options.env).not.toHaveProperty('NEXUS_API_KEY');
        restore(data.game);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
    });

    it('leaves the game unchanged when a CSX script fails', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'throw new Exception();');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');
        const spawnImpl = () => {
            const child = new EventEmitter();
            child.stdout = new EventEmitter();
            child.stderr = new EventEmitter();
            child.kill = vi.fn();
            queueMicrotask(() => child.emit('exit', 1));
            return child;
        };

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            platform: 'win32',
            arch: 'x64',
            undertaleModCliPath: process.execPath,
            spawnImpl
        });

        expect(result.patched).toBe(false);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
        expect(fs.existsSync(path.join(data.game, '.deltamod-community-patch-journal.json'))).toBe(false);
    });

    it('rejects a CSX script changed after approval', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        const script = path.join(data.mod, 'files', 'patch.csx');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(script, 'approved script');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');
        const approvedPlan = buildPatchPlan(data.game, data.mods, ['example-id']);
        fs.writeFileSync(script, 'swapped script');

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            approvedPlan,
            platform: 'win32',
            arch: 'x64',
            undertaleModCliPath: process.execPath,
            spawnImpl: () => { throw new Error('The changed script must not execute.'); }
        });

        expect(result.patched).toBe(false);
        expect(result.log).toMatch(/changed after (?:it|they) were approved/i);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
    });

    it('rejects companion resources changed after approval', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        const resource = path.join(data.mod, 'files', 'resource.txt');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'script');
        fs.writeFileSync(resource, 'approved resource');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');
        const approvedPlan = buildPatchPlan(data.game, data.mods, ['example-id']);
        fs.writeFileSync(resource, 'swapped resource');

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            approvedPlan,
            platform: 'win32',
            arch: 'x64',
            undertaleModCliPath: process.execPath,
            spawnImpl: () => { throw new Error('Changed resources must not execute.'); }
        });

        expect(result.patched).toBe(false);
        expect(result.log).toMatch(/resources.*changed after they were approved/i);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
    });

    it('rejects CSX before execution on unsupported platforms', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'script');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            platform: 'darwin',
            arch: 'arm64',
            spawnImpl: () => { throw new Error('Unsupported platforms must not execute.'); }
        });

        expect(result.patched).toBe(false);
        expect(result.log).toMatch(/not packaged for darwin-arm64/i);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
    });

    it('leaves the game unchanged when UndertaleModCli times out', async () => {
        const data = fixture();
        const gameData = path.join(data.game, 'data.win');
        fs.writeFileSync(gameData, 'FORM-original');
        fs.writeFileSync(path.join(data.mod, 'files', 'patch.csx'), 'while (true) {}');
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="csx" patch="files/patch.csx" to="data.win"/></root>');
        let child;
        const terminateProcessTree = vi.fn();
        const spawnImpl = () => {
            child = new EventEmitter();
            child.stdout = new EventEmitter();
            child.stderr = new EventEmitter();
            child.kill = vi.fn();
            return child;
        };

        const result = await startGamePatch(data.game, data.mods, ['example-id'], null, null, {
            platform: 'win32',
            arch: 'x64',
            undertaleModCliPath: process.execPath,
            spawnImpl,
            terminateProcessTree,
            timeoutMs: 5
        });

        expect(result.patched).toBe(false);
        expect(result.log).toMatch(/timed out/i);
        expect(terminateProcessTree).toHaveBeenCalledWith(child);
        expect(fs.readFileSync(gameData, 'utf8')).toBe('FORM-original');
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
