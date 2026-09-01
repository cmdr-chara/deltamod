const fs = require('node:fs');
const path = require('node:path');

const installTauriAdapter = require('../web/tauri-adapter');
const { buildParity, extractSet } = require('../scripts/tauri-parity/lib/parity');

const RETIRED_RENDERER_COMMANDS = Object.freeze([
    'canReportError',
    'modalTest',
    'openElectronTracer',
    'sampleError'
]);
const PUBLIC_RENDERER_EVENTS = Object.freeze([
    'page',
    'audio',
    'gplog',
    'updateAvailable',
    'themeChange',
    'refresh',
    'finishedPatch',
    'dlmodURL-progress',
    'protocol-download-progress',
    'profile-import-progress',
    'game-import-progress',
    'hash-progress',
    'winResAlert',
    'leave-controller-mode',
    'mod-source-progress',
    'installer-progress',
    'updater-status',
    'updater-progress'
]);
const RETIRED_RENDERER_EVENTS = Object.freeze(['du-progress', 'updateProgress']);
const RETIRED_EVENT_REFERENCES = Object.freeze([
    ...RETIRED_RENDERER_EVENTS,
    'onDDS',
    'onUpdateProgress'
]);

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

function productionRendererJavaScript(webRoot) {
    const excluded = new Set([
        path.join(webRoot, 'preload.js'),
        path.join(webRoot, 'tauri-adapter.js')
    ]);
    const pending = [webRoot];
    const files = [];

    while (pending.length > 0) {
        const directory = pending.pop();
        for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
            const candidate = path.join(directory, entry.name);
            if (entry.isDirectory()) {
                pending.push(candidate);
            } else if (entry.isFile() && candidate.endsWith('.js') && !excluded.has(candidate)) {
                files.push(candidate);
            }
        }
    }

    return files.sort();
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

    it('marks the protocol renderer ready only after every required listener is active', async () => {
        const root = tauriRoot();
        root.__TAURI__.core.invoke.mockImplementation((_command, request) => {
            if (request?.channel === 'protocol:rendererReady' && request.data?.[0] === 'subscribe') {
                return Promise.resolve(7);
            }
            return Promise.resolve(true);
        });
        installTauriAdapter(root);

        root.deltamodBackend.on('page', vi.fn());
        root.deltamodBackend.on('gplog', vi.fn());
        root.deltamodBackend.on('refresh', vi.fn());
        root.deltamodBackend.on('protocol-download-progress', vi.fn());
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();

        const handshakes = root.__TAURI__.core.invoke.mock.calls
            .filter(([, request]) => request?.channel === 'protocol:rendererReady')
            .map(([, request]) => request.data);
        expect(handshakes).toEqual([
            ['subscribe'],
            ['ready', 7]
        ]);
    });

    it('preserves the legacy winResAlert empty-array payload in the bridge and declarations', () => {
        const root = tauriRoot();
        installTauriAdapter(root);
        const callback = vi.fn();
        const payload = [];

        root.preloadAPI.onWRA(callback);
        const [channel, handler] = root.__TAURI__.event.listen.mock.calls.at(-1);
        handler({ payload });

        expect(channel).toBe('winResAlert');
        expect(callback).toHaveBeenCalledWith(payload);
        const declarations = fs.readFileSync(
            path.resolve(__dirname, '..', 'web', 'types', 'preload.d.ts'),
            'utf8'
        );
        expect(declarations).toContain(
            'onWRA(callback: (payload: []) => void): Unsubscribe;'
        );
        expect(declarations).not.toContain(
            'onWRA(callback: (message: string) => void): Unsubscribe;'
        );
    });

    it('provides structured compatibility aliases', async () => {
        const root = tauriRoot();
        installTauriAdapter(root);
        await root.communityAPI.app.version();
        await root.communityAPI.tools.openInstallationInUndertaleModTool('2');
        root.preloadAPI.onHashProgress(vi.fn());
        root.preloadAPI.onGameImportProgress(vi.fn());
        root.preloadAPI.onUpdaterProgress(vi.fn());
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(1, 'backend_invoke', {
            channel: 'version', data: []
        });
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(2, 'backend_invoke', {
            channel: 'undertaleModTool:openInstallation', data: ['2']
        });
        expect(root.__TAURI__.event.listen).toHaveBeenCalledWith('hash-progress', expect.any(Function));
        expect(root.__TAURI__.event.listen).toHaveBeenCalledWith(
            'game-import-progress', expect.any(Function)
        );
        expect(root.__TAURI__.event.listen).toHaveBeenCalledWith(
            'updater-progress', expect.any(Function)
        );
        expect(Object.keys(root.preloadAPI).sort()).toEqual([
            'onPage',
            'onAudio',
            'onGPL',
            'onUpdateAvailable',
            'onUpdaterStatus',
            'onUpdaterProgress',
            'onThemeChange',
            'onRefresh',
            'onFinishedPatch',
            'onDLMODProgress',
            'onProtocolDownloadProgress',
            'onProfileImportProgress',
            'onGameImportProgress',
            'onHashProgress',
            'onWRA',
            'onLeaveControllerMode'
        ].sort());
        expect(root.preloadAPI).not.toHaveProperty('onDDS');
        expect(root.preloadAPI).not.toHaveProperty('onUpdateProgress');
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

    it('only reports renderer-visible Rust-implemented commands as available', () => {
        const root = tauriRoot();
        installTauriAdapter(root);

        const repo = path.resolve(__dirname, '..');
        const report = buildParity({
            preloadPath: path.join(repo, 'web', 'preload.js'),
            rustPath: path.join(repo, 'src-tauri', 'src', 'main.rs')
        });
        expect(report.counts).toEqual({
            electronInvoke: 129,
            electronEvents: 18,
            rustKnown: 129,
            rustImplemented: 123,
            rustUnsupported: 6
        });

        const preloadEvents = report.electron.events.map(event => event.name);
        const adapterEvents = extractSet(
            fs.readFileSync(path.join(repo, 'web', 'tauri-adapter.js'), 'utf8'),
            'allowedEvents'
        ).map(event => event.name);
        expect(preloadEvents).toEqual(PUBLIC_RENDERER_EVENTS);
        expect(adapterEvents).toEqual(PUBLIC_RENDERER_EVENTS);
        for (const event of RETIRED_RENDERER_EVENTS) {
            expect(preloadEvents).not.toContain(event);
            expect(adapterEvents).not.toContain(event);
        }

        const rendererCommands = report.electron.invokes.map(command => command.name).sort();
        const publicRustCommands = report.rust.publicChannels.map(command => command.name).sort();
        expect(rendererCommands).toEqual(publicRustCommands);
        for (const command of RETIRED_RENDERER_COMMANDS) {
            expect(rendererCommands).not.toContain(command);
            expect(publicRustCommands).not.toContain(command);
        }

        const expectedUnsupported = [
            'createInstallLink',
            'gamebanana_downloadAllInCollection',
            'initialize',
            'npsCallback',
            'rebootDev',
            'undertaleModTool:openInstallation'
        ];
        const unsupported = report.rust.publicChannels
            .filter(command => command.classification === 'unsupported')
            .map(command => command.name)
            .sort();
        expect(unsupported).toEqual(expectedUnsupported);

        const unsupportedSet = new Set(expectedUnsupported);
        const expectedImplemented = rendererCommands
            .filter(command => !unsupportedSet.has(command));
        const implemented = report.rust.publicChannels
            .filter(command => command.classification === 'implemented')
            .map(command => command.name)
            .sort();
        expect(implemented).toEqual(expectedImplemented);
        expect(implemented).toHaveLength(123);

        const adapterCommands = extractSet(
            fs.readFileSync(path.join(repo, 'web', 'tauri-adapter.js'), 'utf8'),
            'implementedCommands'
        ).map(command => command.name).sort();
        expect(adapterCommands).toEqual(expectedImplemented);

        for (const command of implemented) {
            expect(root.deltamodBackend.isCommandAvailable(command)).toBe(true);
        }
        const internalCommands = report.excludedInternal.map(command => command.name).sort();
        expect(internalCommands).toEqual([
            'modSources:validateUrl',
            'protocol:parseDeepLink',
            'protocol:planRange',
            'protocol:queueDeepLink',
            'protocol:rendererReady'
        ]);
        for (const command of internalCommands) {
            expect(root.deltamodBackend.isCommandAvailable(command)).toBe(false);
        }
        for (const value of [
            ...unsupported, ...RETIRED_RENDERER_COMMANDS,
            'unknown-command', '', ' ', null, undefined, false, 0, {}, [], ['version'],
            Object('version'), Symbol('version')
        ]) {
            let available;
            expect(() => { available = root.deltamodBackend.isCommandAvailable(value); }).not.toThrow();
            expect(available).toBe(false);
        }
    });

    it('keeps retired renderer commands unavailable and surfaces Rust rejection', async () => {
        const root = tauriRoot();
        root.__TAURI__.core.invoke.mockRejectedValue(
            new Error('TAURI_COMMAND_UNAVAILABLE:unknown')
        );
        installTauriAdapter(root);

        for (const command of RETIRED_RENDERER_COMMANDS) {
            expect(root.deltamodBackend.isCommandAvailable(command)).toBe(false);
            await expect(root.deltamodBackend.invoke(command))
                .rejects.toThrow('TAURI_COMMAND_UNAVAILABLE:unknown');
        }
        expect(root.__TAURI__.core.invoke).toHaveBeenCalledTimes(RETIRED_RENDERER_COMMANDS.length);
    });

    it('has no retired IPC references in production renderer JavaScript', () => {
        const webRoot = path.resolve(__dirname, '..', 'web');
        const files = productionRendererJavaScript(webRoot);
        const references = [];

        expect(files.length).toBeGreaterThan(0);
        for (const filename of files) {
            const source = fs.readFileSync(filename, 'utf8');
            for (const command of RETIRED_RENDERER_COMMANDS) {
                if (source.includes(command)) {
                    references.push(`${path.relative(webRoot, filename)}:${command}`);
                }
            }
        }
        expect(references).toEqual([]);
    });

    it('blocks retired events without disturbing active event subscriptions', () => {
        const root = tauriRoot();
        installTauriAdapter(root);

        for (const event of RETIRED_RENDERER_EVENTS) {
            expect(() => root.deltamodBackend.on(event, vi.fn()))
                .toThrow('Blocked unknown IPC event channel');
        }
        expect(root.__TAURI__.event.listen).not.toHaveBeenCalled();

        root.preloadAPI.onGameImportProgress(vi.fn());
        root.preloadAPI.onUpdaterProgress(vi.fn());
        expect(root.__TAURI__.event.listen).toHaveBeenNthCalledWith(
            1, 'game-import-progress', expect.any(Function)
        );
        expect(root.__TAURI__.event.listen).toHaveBeenNthCalledWith(
            2, 'updater-progress', expect.any(Function)
        );
    });

    it('has no retired event references in production renderer JavaScript or declarations', () => {
        const repo = path.resolve(__dirname, '..');
        const webRoot = path.join(repo, 'web');
        const references = [];

        for (const filename of productionRendererJavaScript(webRoot)) {
            const source = fs.readFileSync(filename, 'utf8');
            for (const reference of RETIRED_EVENT_REFERENCES) {
                if (source.includes(reference)) {
                    references.push(`${path.relative(webRoot, filename)}:${reference}`);
                }
            }
        }
        const declarations = fs.readFileSync(path.join(webRoot, 'types', 'preload.d.ts'), 'utf8');
        for (const api of ['onDDS', 'onUpdateProgress']) {
            if (declarations.includes(api)) references.push(`types/preload.d.ts:${api}`);
        }
        expect(references).toEqual([]);
    });

    it('keeps Rust authoritative for actual invokes and only absorbs exact optional errors', async () => {
        const root = tauriRoot();
        installTauriAdapter(root);

        await root.deltamodBackend.invoke('undertaleModTool:openInstallation', ['2']);
        await root.deltamodBackend.invoke('unknown-command');
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(1, 'backend_invoke', {
            channel: 'undertaleModTool:openInstallation', data: ['2']
        });
        expect(root.__TAURI__.core.invoke).toHaveBeenNthCalledWith(2, 'backend_invoke', {
            channel: 'unknown-command', data: []
        });

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
