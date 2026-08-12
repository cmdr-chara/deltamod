// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const { approvePatchPlan, buildPatchPlan } = require('../node/GamePatching');
const { validatePatchPlanNative, _protocol } = require('../node/security/NativePatchPlanValidation');

const roots = [];

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

function debugBinary() {
    const executable = process.platform === 'win32'
        ? 'deltamod-patch-plan-worker.exe'
        : 'deltamod-patch-plan-worker';
    return path.join(__dirname, '..', 'native', 'target', 'debug', executable);
}

function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-native-plan-'));
    roots.push(root);
    const game = path.join(root, 'game');
    const mods = path.join(root, 'mods');
    const mod = path.join(mods, 'Mod_example');
    fs.mkdirSync(game, { recursive: true });
    fs.mkdirSync(path.join(mod, 'files'), { recursive: true });
    fs.writeFileSync(path.join(mod, 'files', 'source.bin'), 'patch');
    fs.writeFileSync(path.join(mod, 'meta.toml'), '[metadata]\nname="Example"');
    fs.writeFileSync(path.join(mod, '__deltaID.json'), '{"uniqueId":"example-id"}');
    return { root, game, mods, mod };
}

function request(data, patches, platform = 'linux') {
    return {
        schemaVersion: 1,
        gameRoot: data.game,
        platform,
        patches: patches.map(patch => ({
            type: 'override',
            patch: 'files/source.bin',
            to: 'target.bin',
            mappedTarget: 'target.bin',
            modName: 'Example',
            modId: 'example-id',
            modRoot: data.mod,
            ...patch
        }))
    };
}

describe('native patch-plan approval', () => {
    it('strictly parses bounded worker responses', () => {
        expect(_protocol.parseResponse('{"ok":true,"operationCount":1,"patchCount":2,"snapshotCount":3}\n'))
            .toEqual({ operationCount: 1, patchCount: 2, snapshotCount: 3 });
        expect(() => _protocol.parseResponse('{"ok":true,"operationCount":1,"patchCount":2,"snapshotCount":3,"path":"bad"}\n'))
            .toThrow(/invalid success response/i);
        expect(() => _protocol.parseResponse('{}\n')).toThrow(/schema/i);
        expect(() => _protocol.parseResponse('{}\n{}\n')).toThrow(/one JSON/i);
        expect(() => _protocol.parseResponse(`${'x'.repeat(8193)}\n`)).toThrow(/size limit/i);
    });

    it('rejects malformed, unknown-field, and oversized worker input', () => {
        const binary = debugBinary();
        for (const input of [
            '{not-json',
            JSON.stringify({ schemaVersion: 1, gameRoot: 'x', platform: 'linux', patches: [], unknown: true }),
            'x'.repeat(1024 * 1024 + 1)
        ]) {
            const result = spawnSync(binary, [], { input, encoding: 'utf8', windowsHide: true, shell: false });
            expect(result.status).toBe(0);
            expect(JSON.parse(result.stdout)).toMatchObject({ ok: false, code: 'PATCH_PLAN_INVALID' });
            expect(result.stderr).toBe('');
        }
    });

    it('runs the explicit debug worker for all target platform semantics', async () => {
        const data = fixture();
        const binary = debugBinary();
        expect(fs.existsSync(binary)).toBe(true);
        for (const platform of ['win32', 'linux', 'darwin']) {
            await expect(validatePatchPlanNative(request(data, [], platform), { sidecarPath: binary }))
                .resolves.toMatchObject({ operationCount: 0, patchCount: 0 });
        }
    });

    it('enforces Windows case-insensitive and POSIX case-sensitive conflicts', async () => {
        const data = fixture();
        const patches = [
            { to: 'Data/File.bin', mappedTarget: 'Data/File.bin' },
            { type: 'copy', to: 'data/file.bin', mappedTarget: 'data/file.bin' }
        ];
        await expect(validatePatchPlanNative(request(data, patches, 'win32'), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/modified by both/i);
        await expect(validatePatchPlanNative(request(data, patches, 'linux'), { sidecarPath: debugBinary() }))
            .resolves.toMatchObject({ operationCount: 2 });
        await expect(validatePatchPlanNative(request(data, patches, 'darwin'), { sidecarPath: debugBinary() }))
            .resolves.toMatchObject({ operationCount: 2 });
    });

    it('rejects absolute, traversal, encoded, and malformed paths', async () => {
        const data = fixture();
        for (const unsafe of ['../outside', '%252e%252e%252foutside', '/absolute', 'C:\\absolute', '\\\\server\\share', '%zz']) {
            await expect(validatePatchPlanNative(request(data, [{ mappedTarget: unsafe }]), { sidecarPath: debugBinary() }))
                .rejects.toMatchObject({ code: 'PATCH_PLAN_INVALID' });
        }
    });

    it('validates mapped CSX and merge targets and conflicting patch types', async () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.game, 'game.unx'), 'game');
        fs.writeFileSync(path.join(data.mod, 'files', 'script.csx'), 'script');
        const csx = { type: 'csx', patch: 'files/script.csx', to: 'data.win', mappedTarget: 'game.unx' };
        await expect(validatePatchPlanNative(request(data, [csx]), { sidecarPath: debugBinary() }))
            .resolves.toMatchObject({ operationCount: 1 });
        await expect(validatePatchPlanNative(request(data, [{ ...csx, mappedTarget: 'missing.win' }]), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/GameMaker data file|does not exist/i);
        await expect(validatePatchPlanNative(request(data, [{ type: 'xdelta' }]), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/does not exist/i);
        await expect(validatePatchPlanNative(request(data, [{}, { type: 'xdelta' }]), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/direct and merge/i);
    });

    it('rejects source and target hardlinks', async () => {
        const data = fixture();
        fs.linkSync(path.join(data.mod, 'files', 'source.bin'), path.join(data.mod, 'files', 'second.bin'));
        await expect(validatePatchPlanNative(request(data, [{}]), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/hardlink/i);
    });

    it('rejects linked target ancestors', async () => {
        const data = fixture();
        const outside = path.join(data.root, 'outside');
        fs.mkdirSync(outside);
        try {
            fs.symlinkSync(outside, path.join(data.game, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
        } catch (error) {
            if (process.platform === 'win32' && error.code === 'EPERM') return;
            throw error;
        }
        await expect(validatePatchPlanNative(request(data, [{ to: 'linked/target', mappedTarget: 'linked/target' }]), { sidecarPath: debugBinary() }))
            .rejects.toThrow(/link|reparse/i);
    });

    it('uses only the named fallback when an explicitly unavailable worker is requested', async () => {
        const data = fixture();
        fs.writeFileSync(path.join(data.mod, 'modding.xml'), '<root><patch type="override" patch="files/source.bin" to="target.bin"/></root>');
        const plan = buildPatchPlan(data.game, data.mods, ['example-id']);
        await expect(approvePatchPlan(plan, { patchPlanWorkerPath: path.join(data.root, 'missing-worker') }))
            .resolves.toEqual({ operationCount: 1 });
    });

    it('fails closed for present failing workers and oversized input', async () => {
        const data = fixture();
        await expect(validatePatchPlanNative(request(data, [{}]), { sidecarPath: process.execPath }))
            .rejects.toMatchObject({ code: 'PATCH_PLAN_NATIVE_FAILED' });
        const oversized = request(data, [{ modName: 'x'.repeat(1024 * 1024) }]);
        await expect(validatePatchPlanNative(oversized, { sidecarPath: debugBinary() }))
            .rejects.toMatchObject({ code: 'PATCH_PLAN_NATIVE_FAILED' });
    });
});
