const fs = require('fs');
const path = require('path');
const installLinuxMenuAudio = require('../web/linux-menu-audio');

const projectRoot = path.join(__dirname, '..');

function deferred() {
    let resolve;
    let reject;
    const promise = new Promise((res, rej) => {
        resolve = res;
        reject = rej;
    });
    return { promise, resolve, reject };
}

function linuxMenuAudioRoot() {
    const fetches = [];
    const objectUrls = [];
    const revoked = [];
    const switchCalls = [];
    const listeners = new Map();
    let objectUrlId = 0;

    class CompatibleURL extends URL {}
    CompatibleURL.createObjectURL = vi.fn(() => {
        const objectUrl = `blob:deltamod-menu-${++objectUrlId}`;
        objectUrls.push(objectUrl);
        return objectUrl;
    });
    CompatibleURL.revokeObjectURL = vi.fn(objectUrl => revoked.push(objectUrl));

    const audio = {
        _src: '',
        currentSrc: '',
        dataset: {},
        paused: true,
        readyState: 0,
        currentTime: 0,
        loop: true,
        volume: 0.5,
        preload: 'none',
        get src() {
            return this._src;
        },
        set src(value) {
            this._src = String(value);
        },
        pause: vi.fn(function pause() {
            this.paused = true;
        }),
        removeAttribute: vi.fn(function removeAttribute(name) {
            if (name === 'src') this._src = '';
        }),
        load: vi.fn(function load() {
            this.readyState = 0;
        }),
        play: vi.fn(function bridgePlay() {
            if (!this.src.startsWith('blob:')) {
                const error = new Error(`Underlying media bridge received non-Blob source: ${this.src}`);
                error.code = 'DELTAMOD_MEDIA_SOURCE_CHANGED';
                return Promise.reject(error);
            }
            this.paused = false;
            this.readyState = 2;
            return Promise.resolve('played');
        })
    };

    const root = {
        DeltamodLinuxCompat: { isLinuxTauri: true },
        audio,
        currentAudioSource: '',
        location: { href: 'tauri://localhost/index.html' },
        URL: CompatibleURL,
        AbortController,
        DOMException,
        console: { warn: vi.fn() },
        fetch: vi.fn((source, options) => {
            const pending = deferred();
            fetches.push({ source, options, ...pending });
            return pending.promise;
        }),
        configureMenuAudioPlayback: vi.fn(),
        switchMenuAudioSource: vi.fn(function switchMenuAudioSource(source) {
            switchCalls.push(source);
            audio.pause();
            root.currentAudioSource = source;
            audio.src = source;
        }),
        releaseAudioBuffer: vi.fn(function releaseAudioBuffer() {
            audio.pause();
            audio.currentTime = 0;
            audio.removeAttribute('src');
            audio.load();
            root.currentAudioSource = '';
        }),
        addEventListener: vi.fn((name, callback) => listeners.set(name, callback))
    };

    return {
        root,
        audio,
        fetches,
        objectUrls,
        revoked,
        switchCalls,
        listeners,
        CompatibleURL
    };
}

async function completeFetch(entry, type = 'audio/mpeg') {
    entry.resolve({
        ok: true,
        status: 200,
        blob: () => Promise.resolve({ type })
    });
    await Promise.resolve();
    await Promise.resolve();
}

describe('Linux menu audio request coordination', () => {
    it('coalesces duplicate play() calls while one Blob request is loading', async () => {
        const { root, audio, fetches, objectUrls } = linuxMenuAudioRoot();
        const source = 'themeprot://chara/chara-theme.mp3';
        const nativeBridgePlay = audio.play;

        expect(installLinuxMenuAudio(root)).toBe('active');
        root.switchMenuAudioSource(source);
        const firstPlay = audio.play();
        const secondPlay = audio.play();

        expect(fetches).toHaveLength(1);
        await completeFetch(fetches[0]);
        await expect(firstPlay).resolves.toBe('played');
        await expect(secondPlay).resolves.toBe('played');

        expect(objectUrls).toHaveLength(1);
        expect(audio.preload).toBe('auto');
        expect(nativeBridgePlay).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxMenuAudio.snapshot().coalescedPlays).toBe(1);
    });

    it('bypasses the patched media play method after assigning its Blob URL', async () => {
        const { root, audio, fetches } = linuxMenuAudioRoot();
        const patchedPlay = audio.play;
        const nativePlay = vi.fn(function nativePlay(element) {
            if (!element.src.startsWith('blob:')) {
                return Promise.reject(new Error(`Unexpected source: ${element.src}`));
            }
            element.paused = false;
            element.readyState = 2;
            return Promise.resolve('native-played');
        });
        root.DeltamodLinuxCompat.playNative = nativePlay;
        const source = 'themeprot://chara/chara-theme.mp3';

        installLinuxMenuAudio(root);
        root.switchMenuAudioSource(source);
        const play = audio.play();

        expect(fetches).toHaveLength(1);
        await completeFetch(fetches[0]);
        await expect(play).resolves.toBe('native-played');
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(patchedPlay).not.toHaveBeenCalled();
    });

    it('falls back to a decoded Web Audio buffer when WebKit rejects MP3 media', async () => {
        const { root, audio, fetches } = linuxMenuAudioRoot();
        const nativeError = Object.assign(new Error('The operation is not supported.'), {
            name: 'NotSupportedError',
            code: 4
        });
        const gain = {
            gain: { value: 0 },
            connect: vi.fn()
        };
        const sourceNode = {
            loop: false,
            connect: vi.fn(() => gain),
            start: vi.fn(),
            stop: vi.fn(),
            disconnect: vi.fn()
        };
        const context = {
            state: 'running',
            destination: {},
            resume: vi.fn(() => Promise.resolve()),
            decodeAudioData: vi.fn(() => Promise.resolve({ duration: 177 })),
            createGain: vi.fn(() => gain),
            createBufferSource: vi.fn(() => sourceNode)
        };
        gain.connect.mockReturnValue(context.destination);
        root.DeltamodLinuxCompat.playNative = vi.fn(() => Promise.reject(nativeError));
        root.AudioContext = vi.fn(function AudioContext() {
            return context;
        });
        root.setInterval = vi.fn(() => 1);
        root.clearInterval = vi.fn();
        const source = 'themeprot://chara/chara-theme.mp3';

        installLinuxMenuAudio(root);
        root.switchMenuAudioSource(source);
        const play = audio.play();

        expect(fetches).toHaveLength(1);
        fetches[0].resolve({
            ok: true,
            status: 200,
            blob: () => Promise.resolve({
                type: 'audio/mpeg',
                arrayBuffer: () => Promise.resolve(new ArrayBuffer(8))
            })
        });

        await expect(play).resolves.toBe('web-audio-played');
        expect(context.decodeAudioData).toHaveBeenCalledTimes(1);
        expect(sourceNode.start).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxMenuAudio.snapshot()).toMatchObject({
            webAudioFallbacks: 1,
            webAudioDecodeLoads: 1,
            lastError: null
        });
    });

    it('does not reset the player when switchMenuAudioSource() repeats the active source', () => {
        const { root, audio, switchCalls } = linuxMenuAudioRoot();
        const source = 'themeprot://chara/chara-theme.mp3';

        installLinuxMenuAudio(root);
        root.switchMenuAudioSource(source);
        root.switchMenuAudioSource(source);

        expect(switchCalls).toEqual([source]);
        expect(audio.pause).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxMenuAudio.snapshot().sameSourceSwitches).toBe(1);
    });

    it('invalidates an obsolete request when the menu source changes in flight', async () => {
        const { root, audio, fetches, revoked } = linuxMenuAudioRoot();
        const firstSource = 'themeprot://theme/first.mp3';
        const secondSource = 'themeprot://theme/second.mp3';

        installLinuxMenuAudio(root);
        root.switchMenuAudioSource(firstSource);
        const firstPlay = audio.play();
        const firstPlayRejection = expect(firstPlay).rejects.toMatchObject({ name: 'AbortError' });
        root.switchMenuAudioSource(secondSource);
        const secondPlay = audio.play();

        expect(fetches).toHaveLength(2);
        expect(fetches[0].options.signal.aborted).toBe(true);

        await completeFetch(fetches[1]);
        await expect(secondPlay).resolves.toBe('played');
        await completeFetch(fetches[0]);
        await firstPlayRejection;

        expect(revoked).toHaveLength(1);
        expect(root.console.warn).not.toHaveBeenCalledWith(
            expect.stringContaining('DELTAMOD_MEDIA_SOURCE_CHANGED'),
            expect.anything()
        );
        expect(root.DeltamodLinuxMenuAudio.snapshot().supersededRequests).toBe(1);
    });

    it('coalesces the initial video fallback and page-navigation music request', async () => {
        const { root, audio, fetches, switchCalls } = linuxMenuAudioRoot();
        const source = 'tauri://localhost/web/themes/mus/chara-theme.mp3';
        const nativeBridgePlay = audio.play;

        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const fallbackSource = renderer.slice(
            renderer.indexOf('async function fallBackFromThemeVideo'),
            renderer.indexOf('window._onClosePage')
        );
        const renderPageSource = renderer.slice(
            renderer.indexOf('async function renderPage'),
            renderer.indexOf('/**\n * ==========================================\n * Global Window Listeners')
        );
        expect(fallbackSource).toContain('switchMenuAudioSource(');
        expect(fallbackSource).toContain('audio.play()');
        expect(renderPageSource).toContain('switchMenuAudioSource(');
        expect(renderPageSource).toContain('audio.play()');

        installLinuxMenuAudio(root);

        // fallBackFromThemeVideo() reaches the same source while renderPage()
        // is also processing AUDIO[mainTheme.mp3] during initial navigation.
        root.switchMenuAudioSource(source);
        const fallbackPlay = audio.play();
        root.switchMenuAudioSource(source);
        const navigationPlay = audio.play();

        expect(switchCalls).toEqual([source]);
        expect(fetches).toHaveLength(1);

        await completeFetch(fetches[0]);
        await Promise.all([fallbackPlay, navigationPlay]);

        expect(nativeBridgePlay).toHaveBeenCalledTimes(1);
        expect(audio.paused).toBe(false);
        expect(audio.readyState).toBeGreaterThanOrEqual(2);
        expect(root.DeltamodLinuxMenuAudio.snapshot().lastError).toBeNull();
    });

    it('keeps the active Blob URL until a real source release or page shutdown', async () => {
        const { root, audio, fetches, revoked, listeners } = linuxMenuAudioRoot();
        const source = 'themeprot://chara/chara-theme.mp3';

        installLinuxMenuAudio(root);
        root.switchMenuAudioSource(source);
        const play = audio.play();
        await completeFetch(fetches[0]);
        await play;

        expect(revoked).toEqual([]);
        await audio.play();
        root.switchMenuAudioSource(source);
        expect(revoked).toEqual([]);

        root.releaseAudioBuffer();
        expect(revoked).toEqual(['blob:deltamod-menu-1']);

        listeners.get('pagehide')();
        expect(revoked).toEqual(['blob:deltamod-menu-1']);
    });

    it('is loaded after index.js but before the remaining Linux runtime polish', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const startup = fs.readFileSync(path.join(projectRoot, 'web', 'modules', 'startup.js'), 'utf8');
        expect(html).toContain('<script src="./modules/startup.js"></script>');
        expect(startup).toContain("'./linux-menu-audio.js'");
        expect(startup).toContain("await load('index.js')");
        expect(startup.indexOf("'./linux-menu-audio.js'"))
            .toBeGreaterThan(startup.indexOf("'index.js'"));
        expect(startup.indexOf("'./linux-menu-audio.js'"))
            .toBeLessThan(startup.indexOf("'./linux-runtime-polish.js'"));
    });

    it('does nothing outside Linux Tauri', () => {
        const { root, audio } = linuxMenuAudioRoot();
        const originalPlay = audio.play;
        root.DeltamodLinuxCompat = undefined;

        expect(installLinuxMenuAudio(root)).toBe('inactive');
        expect(audio.play).toBe(originalPlay);
        expect(root.DeltamodLinuxMenuAudio).toBeUndefined();
    });
});
