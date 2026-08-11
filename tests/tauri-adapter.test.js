const installTauriAdapter = require('../web/tauri-adapter');

function deferred() {
    let resolve;
    const promise = new Promise(done => { resolve = done; });
    return { promise, resolve };
}

function tauriRoot(overrides = {}) {
    const invoke = vi.fn(() => Promise.resolve(null));
    const listen = vi.fn(() => Promise.resolve(vi.fn()));
    return {
        location: { href: 'tauri://localhost/index.html' },
        console: { error: vi.fn() },
        __TAURI__: { core: { invoke }, event: { listen } },
        ...overrides
    };
}

describe('Tauri browser adapter', () => {
    it('does nothing when the Electron preload bridge is present', () => {
        const backend = {};
        const root = tauriRoot({ deltamodBackend: backend, communityAPI: {}, preloadAPI: {} });
        expect(installTauriAdapter(root)).toBe('electron');
        expect(root.deltamodBackend).toBe(backend);
        expect(root.__TAURI__.core.invoke).not.toHaveBeenCalled();
    });

    it('does nothing in an ordinary browser and installs in Tauri', () => {
        expect(installTauriAdapter({})).toBe('browser');
        const root = tauriRoot();
        expect(installTauriAdapter(root)).toBe('tauri');
        expect(root.deltamodBackend).toEqual(expect.objectContaining({
            invoke: expect.any(Function),
            invokeOptional: expect.any(Function),
            isCommandAvailable: expect.any(Function),
            on: expect.any(Function),
            assetUrl: expect.any(Function)
        }));
    });

    it('uses the single Rust command with the expected payload shape', async () => {
        const root = tauriRoot();
        installTauriAdapter(root);
        await root.deltamodBackend.invoke('version', ['ignored']);
        expect(root.__TAURI__.core.invoke).toHaveBeenCalledWith('backend_invoke', {
            channel: 'version', data: ['ignored']
        });
        await expect(root.deltamodBackend.invoke('version', {})).rejects.toThrow('payload must be an array');
    });

    it('forwards exact event payloads and tears down after an early unsubscribe', async () => {
        const pending = deferred();
        const root = tauriRoot();
        root.__TAURI__.event.listen.mockReturnValue(pending.promise);
        installTauriAdapter(root);
        const callback = vi.fn();
        const unsubscribe = root.deltamodBackend.on('themeChange', callback);
        const tauriHandler = root.__TAURI__.event.listen.mock.calls[0][1];
        const unlisten = vi.fn();

        tauriHandler({ payload: { exact: true }, id: 4 });
        expect(callback).toHaveBeenCalledWith({ exact: true });
        unsubscribe();
        pending.resolve(unlisten);
        await pending.promise;
        await Promise.resolve();
        expect(unlisten).toHaveBeenCalledTimes(1);
        unsubscribe();
        expect(unlisten).toHaveBeenCalledTimes(1);
    });

    it('provides structured compatibility aliases', async () => {
        const root = tauriRoot();
        installTauriAdapter(root);
        await root.communityAPI.app.version();
        await root.communityAPI.tools.openInstallationInUndertaleModTool('2');
        root.preloadAPI.onHashProgress(vi.fn());
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(1, 'backend_invoke', {
            channel: 'version', data: []
        });
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(2, 'backend_invoke', {
            channel: 'undertaleModTool:openInstallation', data: ['2']
        });
        expect(root.__TAURI__.event.listen).toHaveBeenCalledWith('hash-progress', expect.any(Function));
    });

    it('keeps app assets static and maps scoped theme and packet assets', () => {
        const root = tauriRoot();
        installTauriAdapter(root);
        expect(root.deltamodBackend.assetUrl('app', 'web/img/Dark Theme.png'))
            .toBe('tauri://localhost/img/Dark%20Theme.png');
        expect(root.deltamodBackend.assetUrl('theme', 'img/theme.png'))
            .toBe('themeprot://asset/img/theme.png');
        expect(root.deltamodBackend.assetUrl('packet', 'pack-1/image/icon.png'))
            .toBe('packet://pack-1/image/icon.png');
        for (const unsafe of ['../secret', '%252e%252e/secret', '/absolute', 'C:/secret', 'a\\b']) {
            expect(() => root.deltamodBackend.assetUrl('app', unsafe)).toThrow();
        }
    });

    it('surfaces explicit unsupported command errors without translating them', async () => {
        const root = tauriRoot();
        root.__TAURI__.core.invoke.mockRejectedValue(new Error('TAURI_COMMAND_UNAVAILABLE:startGame'));
        installTauriAdapter(root);
        await expect(root.deltamodBackend.invoke('startGame'))
            .rejects.toThrow('TAURI_COMMAND_UNAVAILABLE:startGame');
    });

    it('gates Rust-declared unavailable controls and only absorbs their exact optional error', async () => {
        const root = tauriRoot();
        installTauriAdapter(root);

        expect(root.deltamodBackend.isCommandAvailable('version')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('loginGamebanana')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('getGamebananaUserinfo')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('isCMode')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('cmode-on')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('cmode-off')).toBe(true);
        expect(root.deltamodBackend.isCommandAvailable('modSources:downloadNexus')).toBe(false);
        expect(root.deltamodBackend.isCommandAvailable('undertaleModTool:openInstallation')).toBe(false);

        root.__TAURI__.core.invoke.mockRejectedValueOnce(
            new Error('TAURI_COMMAND_UNAVAILABLE:start-update')
        );
        await expect(root.deltamodBackend.invokeOptional('start-update', [], false))
            .resolves.toBe(false);

        root.__TAURI__.core.invoke.mockRejectedValueOnce(new Error('disk failure'));
        await expect(root.deltamodBackend.invokeOptional('start-update', [], false))
            .rejects.toThrow('disk failure');

        root.__TAURI__.core.invoke.mockRejectedValueOnce(
            new Error('TAURI_COMMAND_UNAVAILABLE:ignore-update')
        );
        await expect(root.deltamodBackend.invokeOptional('start-update', [], false))
            .rejects.toThrow('TAURI_COMMAND_UNAVAILABLE:ignore-update');

        root.__TAURI__.core.invoke.mockRejectedValueOnce(
            new Error('TAURI_COMMAND_UNAVAILABLE:modSources:cancelNexusSso')
        );
        await expect(root.communityAPI.modSources.cancelNexusSso()).resolves.toBe(false);
    });
});
