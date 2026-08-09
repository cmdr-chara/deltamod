const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const preloadSource = fs.readFileSync(path.join(__dirname, '..', 'web', 'preload.js'), 'utf8');

function loadPreload() {
    const exposed = {};
    const ipcRenderer = {
        invoke: vi.fn((channel, data) => Promise.resolve({ channel, data })),
        on: vi.fn(),
        removeListener: vi.fn()
    };
    const contextBridge = {
        exposeInMainWorld: vi.fn((name, api) => {
            exposed[name] = api;
        })
    };

    vm.runInNewContext(preloadSource, {
        require: moduleName => {
            if (moduleName !== 'electron') throw new Error(`Unexpected module: ${moduleName}`);
            return { contextBridge, ipcRenderer };
        },
        navigator: {},
        console
    });

    return { contextBridge, exposed, ipcRenderer };
}

describe('platform-neutral preload backend', () => {
    it('exposes the neutral contract without expanding existing APIs', () => {
        const { exposed } = loadPreload();

        expect(Object.keys(exposed).sort()).toEqual([
            'communityAPI',
            'deltamodBackend',
            'electronAPI',
            'preloadAPI'
        ]);
        expect(Object.keys(exposed.deltamodBackend).sort()).toEqual([
            'assetUrl', 'invoke', 'invokeOptional', 'isCommandAvailable', 'on'
        ]);
        expect(Object.keys(exposed.electronAPI)).toEqual(['invoke']);
        expect(exposed).not.toHaveProperty('logElectronAPI');
        expect(exposed.communityAPI.modSources).not.toHaveProperty('setNexusKey');
    });

    it('invokes allowlisted channels and blocks unknown channels', async () => {
        const { exposed, ipcRenderer } = loadPreload();

        await expect(exposed.deltamodBackend.invoke('version')).resolves.toEqual({
            channel: 'version',
            data: []
        });
        expect(ipcRenderer.invoke).toHaveBeenCalledWith('version', []);
        await expect(exposed.deltamodBackend.invoke('shell:open', ['https://example.com']))
            .rejects.toThrow('Blocked unknown IPC channel');
        expect(ipcRenderer.invoke).toHaveBeenCalledTimes(1);
    });

    it('keeps Electron optional commands available and preserves real failures', async () => {
        const { exposed, ipcRenderer } = loadPreload();

        expect(exposed.deltamodBackend.isCommandAvailable('loginGamebanana')).toBe(true);
        await expect(exposed.deltamodBackend.invokeOptional('loginGamebanana', [], false))
            .resolves.toEqual({ channel: 'loginGamebanana', data: [] });
        expect(ipcRenderer.invoke).toHaveBeenCalledWith('loginGamebanana', []);

        ipcRenderer.invoke.mockRejectedValueOnce(new Error('login failed'));
        await expect(exposed.deltamodBackend.invokeOptional('loginGamebanana', [], false))
            .rejects.toThrow('login failed');
    });

    it('subscribes only to allowlisted events and returns an unsubscribe function', () => {
        const { exposed, ipcRenderer } = loadPreload();
        const callback = vi.fn();
        const unsubscribe = exposed.deltamodBackend.on('themeChange', callback);
        const listener = ipcRenderer.on.mock.calls.find(([channel]) => channel === 'themeChange')[1];

        listener({ sender: 'not exposed' }, { name: 'dark' });
        expect(callback).toHaveBeenCalledWith({ name: 'dark' });
        unsubscribe();
        expect(ipcRenderer.removeListener).toHaveBeenCalledWith('themeChange', listener);
        expect(() => exposed.deltamodBackend.on('arbitrary-event', callback))
            .toThrow('Blocked unknown IPC event channel');
    });

    it('constructs the existing custom-protocol asset URL forms', () => {
        const { exposed } = loadPreload();
        const { assetUrl } = exposed.deltamodBackend;

        expect(assetUrl('app', 'web/img/mod-placeholder.png'))
            .toBe('deltapack://web/img/mod-placeholder.png');
        expect(assetUrl('theme', 'img/Dark Theme.png'))
            .toBe('themeprot://img/Dark%20Theme.png');
        expect(assetUrl('packet', 'mod-id/icon.png')).toBe('packet://mod-id/icon.png');
    });

    it.each([
        ['unknown', 'web/image.png'],
        ['app', ''],
        ['app', '/absolute.png'],
        ['app', 'C:/absolute.png'],
        ['app', '../outside.png'],
        ['app', 'web/../outside.png'],
        ['app', 'web\\..\\outside.png'],
        ['app', '%2e%2e/outside.png'],
        ['app', '%252e%252e/outside.png'],
        ['app', '%2Fabsolute.png'],
        ['app', 'https://example.com/image.png'],
        ['app', 'https%3A%2F%2Fexample.com/image.png'],
        ['app', 'web/%00image.png'],
        ['app', 'web/%ZZ.png']
    ])('rejects unsafe asset input %s %s', (kind, assetPath) => {
        const { exposed } = loadPreload();

        expect(() => exposed.deltamodBackend.assetUrl(kind, assetPath)).toThrow();
    });
});
