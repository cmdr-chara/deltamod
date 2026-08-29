const fs = require('fs');
const path = require('path');
const installMediaCompatibility = require('../web/media-compat');

const projectRoot = path.join(__dirname, '..');

function linuxTauriRoot({ mode = null, codecSupport = 'probably' } = {}) {
    const nativePlay = vi.fn(function play() {
        this.nativePlayCount = (this.nativePlayCount || 0) + 1;
        return Promise.resolve('played');
    });
    const classes = new Set();
    const storage = new Map();
    if (mode) storage.set('deltamodLinuxMediaMode', mode);

    class FakeMediaElement {
        constructor(source = '', tagName = 'AUDIO') {
            this.src = source;
            this.currentSrc = '';
            this.tagName = tagName;
            this.id = '';
            this.dataset = {};
            this.loop = false;
            this.preload = 'none';
            this.listeners = new Map();
        }

        getAttribute(name) {
            return name === 'src' ? this.src : null;
        }

        addEventListener(name, callback) {
            const listeners = this.listeners.get(name) || [];
            listeners.push(callback);
            this.listeners.set(name, listeners);
        }

        emit(name) {
            for (const listener of this.listeners.get(name) || []) listener();
        }

        pause() {}
        removeAttribute(name) {
            if (name === 'src') this.src = '';
        }
        load() {
            this.emit('emptied');
        }
        canPlayType() { return codecSupport; }
    }
    FakeMediaElement.prototype.play = nativePlay;

    class CompatibleURL extends URL {}
    let objectUrlId = 0;
    CompatibleURL.createObjectURL = vi.fn(() => `blob:deltamod-media-${++objectUrlId}`);
    CompatibleURL.revokeObjectURL = vi.fn();

    return {
        nativePlay,
        classes,
        storage,
        root: {
            navigator: { platform: 'Linux x86_64', userAgent: 'WebKit Linux' },
            location: { href: 'tauri://localhost/index.html' },
            localStorage: {
                getItem: vi.fn(key => storage.get(key) || null),
                setItem: vi.fn((key, value) => storage.set(key, String(value)))
            },
            document: {
                documentElement: {
                    dataset: {},
                    classList: {
                        add: vi.fn(name => classes.add(name)),
                        remove: vi.fn(name => classes.delete(name)),
                        toggle: vi.fn((name, enabled) => enabled ? classes.add(name) : classes.delete(name))
                    }
                }
            },
            __TAURI__: { core: {} },
            HTMLMediaElement: FakeMediaElement,
            URL: CompatibleURL,
            fetch: vi.fn(() => Promise.resolve({
                ok: true,
                status: 200,
                blob: () => Promise.resolve({ type: 'application/octet-stream' })
            })),
            console: { warn: vi.fn() },
            addEventListener: vi.fn(),
            setTimeout: vi.fn(callback => callback())
        },
        FakeMediaElement,
        CompatibleURL
    };
}

function themeVideo(FakeMediaElement) {
    const video = new FakeMediaElement(
        'tauri://localhost/themes/video/chara-theme.mp4',
        'VIDEO'
    );
    video.id = 'theme-background-video';
    video.dataset.source = video.src;
    return video;
}

describe('Linux WebKitGTK compatibility', () => {
    it('bridges custom-scheme audio without refetching the same element source', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot();

        expect(installMediaCompatibility(root)).toBe('active');
        const audio = new FakeMediaElement('tauri://localhost/audio/rew.mp3', 'AUDIO');

        await expect(audio.play()).resolves.toBe('played');
        expect(root.fetch).toHaveBeenCalledWith('tauri://localhost/audio/rew.mp3');
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(audio.src).toBe('blob:deltamod-media-1');
        expect(audio.preload).toBe('auto');
        expect(root.DeltamodLinuxCompat.snapshot().audioBlobLoads).toBe(1);

        await audio.play();
        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(nativePlay).toHaveBeenCalledTimes(2);
    });

    it('exposes the unpatched media play method to companion bridges', async () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('blob:deltamod-native', 'AUDIO');

        await expect(root.DeltamodLinuxCompat.playNative(audio)).resolves.toBe('played');
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(audio.nativePlayCount).toBe(1);
    });

    it('waits for Blob media readiness before invoking native playback', async () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        const timers = [];
        root.setTimeout = vi.fn(callback => {
            timers.push(callback);
            return timers.length;
        });
        root.clearTimeout = vi.fn();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('blob:deltamod-native', 'AUDIO');
        audio.readyState = 0;

        const play = root.DeltamodLinuxCompat.playNativeWhenReady(audio);
        await Promise.resolve();
        expect(nativePlay).not.toHaveBeenCalled();

        audio.readyState = 4;
        audio.emit('canplay');
        await expect(play).resolves.toBe('played');
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(root.clearTimeout).toHaveBeenCalledWith(1);
    });

    it('shares tiny app SFX blobs across separate transient Audio elements', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const first = new FakeMediaElement('tauri://localhost/audio/htmlalert.mp3', 'AUDIO');
        const second = new FakeMediaElement('tauri://localhost/audio/htmlalert.mp3', 'AUDIO');

        await first.play();
        await second.play();

        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        const snapshot = root.DeltamodLinuxCompat.snapshot();
        expect(snapshot.sharedAudioBlobLoads).toBe(1);
        expect(snapshot.sharedAudioCacheHits).toBe(1);
    });

    it('revokes stale non-shared object URLs when an audio element changes source', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('themeprot://asset/ch5.mp3', 'AUDIO');

        await audio.play();
        audio.src = 'themeprot://asset/ch6.mp3';
        await audio.play();

        expect(root.fetch).toHaveBeenNthCalledWith(1, 'themeprot://asset/ch5.mp3');
        expect(root.fetch).toHaveBeenNthCalledWith(2, 'themeprot://asset/ch6.mp3');
        expect(CompatibleURL.revokeObjectURL).toHaveBeenCalledWith('blob:deltamod-media-1');
        expect(audio.dataset.deltamodOriginalMediaSource).toBe('themeprot://asset/ch6.mp3');
    });

    it('releases one-shot non-shared media blobs after playback ends', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('themeprot://asset/ch5.mp3', 'AUDIO');

        await audio.play();
        audio.emit('ended');

        expect(CompatibleURL.revokeObjectURL).toHaveBeenCalledWith('blob:deltamod-media-1');
        expect(root.DeltamodLinuxCompat.snapshot().revokedObjectUrls).toBe(1);
    });

    it('releases a bridged audio element when load() empties it', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('themeprot://asset/ch5.mp3', 'AUDIO');

        await audio.play();
        audio.emit('emptied');

        expect(CompatibleURL.revokeObjectURL).toHaveBeenCalledWith('blob:deltamod-media-1');
    });

    it('blocks theme background video immediately in auto mode', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot();
        installMediaCompatibility(root);
        const video = themeVideo(FakeMediaElement);

        await expect(video.play()).rejects.toMatchObject({
            name: 'NotSupportedError',
            code: 'DELTAMOD_LINUX_THEME_VIDEO_DISABLED'
        });
        expect(root.fetch).not.toHaveBeenCalled();
        expect(CompatibleURL.createObjectURL).not.toHaveBeenCalled();
        expect(nativePlay).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxCompat.forcesPosterVideo()).toBe(true);
        expect(root.DeltamodLinuxCompat.snapshot().videoBlocks).toBe(1);
    });

    it('blocks theme background video in performance mode too', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot({ mode: 'performance' });
        installMediaCompatibility(root);
        const video = themeVideo(FakeMediaElement);

        await expect(video.play()).rejects.toMatchObject({
            code: 'DELTAMOD_LINUX_THEME_VIDEO_DISABLED'
        });
        expect(root.fetch).not.toHaveBeenCalled();
        expect(CompatibleURL.createObjectURL).not.toHaveBeenCalled();
        expect(nativePlay).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxCompat.forcesPosterVideo()).toBe(true);
    });

    it('buffers theme video once in quality mode for stable WebKitGTK playback', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot({ mode: 'quality' });
        installMediaCompatibility(root);
        const video = themeVideo(FakeMediaElement);

        await expect(video.play()).resolves.toBe('played');
        expect(root.fetch).toHaveBeenCalledWith('tauri://localhost/themes/video/chara-theme.mp4');
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxCompat.forcesPosterVideo()).toBe(false);
        expect(video.preload).toBe('auto');
        const snapshot = root.DeltamodLinuxCompat.snapshot();
        expect(snapshot.videoDirectPlays).toBe(0);
        expect(snapshot.videoBlobLoads).toBe(1);

        await video.play();
        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(nativePlay).toHaveBeenCalledTimes(2);

        // A cancelled custom-scheme load may report emptied after the Blob
        // source is installed. That stale event must not discard the bridge.
        video.src = '';
        video.emit('emptied');
        video.src = 'blob:deltamod-media-1';
        await video.play();
        expect(root.fetch).toHaveBeenCalledTimes(1);
    });

    it('keeps non-theme custom-scheme video native in every mode', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot();
        installMediaCompatibility(root);
        const video = new FakeMediaElement('packet://pack/video/preview.mp4', 'VIDEO');

        await expect(video.play()).resolves.toBe('played');
        expect(root.fetch).not.toHaveBeenCalled();
        expect(CompatibleURL.createObjectURL).not.toHaveBeenCalled();
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxCompat.snapshot().videoBlobLoads).toBe(0);
    });

    it('rejects H.264/AAC quality video early when WebKitGTK reports no codec support', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot({
            mode: 'quality',
            codecSupport: ''
        });
        installMediaCompatibility(root);
        const video = themeVideo(FakeMediaElement);

        await expect(video.play()).rejects.toMatchObject({
            name: 'NotSupportedError',
            code: 'DELTAMOD_LINUX_VIDEO_CODEC_UNAVAILABLE'
        });
        expect(root.fetch).not.toHaveBeenCalled();
        expect(CompatibleURL.createObjectURL).not.toHaveBeenCalled();
        expect(nativePlay).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxCompat.snapshot().videoCodecBlocks).toBe(1);
        expect(root.console.warn).toHaveBeenCalledWith(expect.stringContaining('gst-libav'));
    });

    it('persists mode and keeps expensive effects disabled outside quality mode', () => {
        const { root, classes, storage } = linuxTauriRoot();
        installMediaCompatibility(root);

        expect(classes.has('deltamod-linux-webkit')).toBe(true);
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(true);
        expect(classes.has('deltamod-linux-performance')).toBe(false);
        expect(root.DeltamodLinuxCompat.usesReducedEffects()).toBe(true);

        root.DeltamodLinuxCompat.setMode('performance');
        expect(storage.get('deltamodLinuxMediaMode')).toBe('performance');
        expect(classes.has('deltamod-linux-performance')).toBe(true);
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(true);
        expect(root.DeltamodLinuxCompat.usesReducedEffects()).toBe(true);

        root.DeltamodLinuxCompat.setMode('quality');
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(false);
        expect(classes.has('deltamod-linux-performance')).toBe(false);
        expect(root.DeltamodLinuxCompat.usesReducedEffects()).toBe(false);

        root.DeltamodLinuxCompat.setMode('auto');
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(true);
        expect(root.DeltamodLinuxCompat.usesReducedEffects()).toBe(true);
    });

    it('does not patch media playback outside Linux Tauri', () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        root.navigator = { platform: 'MacIntel', userAgent: 'WebKit macOS' };

        expect(installMediaCompatibility(root)).toBe('inactive');
        expect(FakeMediaElement.prototype.play).toBe(nativePlay);
        expect(root.DeltamodLinuxCompat).toBeUndefined();
    });

    it('loads Linux media and runtime polish under blob-enabled HTML and Tauri CSPs', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const tauriConfig = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'src-tauri', 'tauri.conf.json'),
            'utf8'
        ));
        const csp = tauriConfig.app.security.csp;

        expect(html).toContain("media-src 'self' blob: deltapack: themeprot: packet:");
        expect(html).toContain('<link rel="stylesheet" href="linux-webkit-compat.css">');
        expect(html).toContain('<script src="./media-compat.js"></script>');
        expect(html).toContain("'./linux-runtime-polish.js'");
        expect(html.indexOf('./media-compat.js')).toBeGreaterThan(html.indexOf('./tauri-adapter.js'));
        expect(html.indexOf("'./linux-runtime-polish.js'")).toBeGreaterThan(html.indexOf("'index.js'"));
        expect(csp['media-src']).toContain('blob:');
        expect(csp['connect-src']).toContain('themeprot:');
        expect(csp['connect-src']).toContain('packet:');
    });

    it('keeps body text antialiased while reducing Linux-only composition effects', () => {
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'linux-webkit-compat.css'), 'utf8');

        expect(css).toContain('html.deltamod-linux-webkit body');
        expect(css).toContain('font-family: system-ui, "Segoe UI", sans-serif;');
        expect(css).toContain('-webkit-font-smoothing: antialiased;');
        expect(css).toContain('html.deltamod-linux-webkit .setting-title');
        expect(css).toContain('-webkit-font-smoothing: none;');
        expect(css).not.toContain('html.deltamod-linux-webkit * {');
        expect(css).toContain('html.deltamod-linux-reduced-effects');
        expect(css).toContain('backdrop-filter: none !important;');
        expect(css).toContain('.ingranaggio-wheel');
        expect(css).toContain('html.deltamod-linux-performance .theme-background-video');
        expect(css).toContain('color-scheme: dark;');
        expect(css).toContain('-webkit-appearance: none;');
    });
});
