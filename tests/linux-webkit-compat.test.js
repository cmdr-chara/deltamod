const fs = require('fs');
const path = require('path');
const installMediaCompatibility = require('../web/media-compat');

const projectRoot = path.join(__dirname, '..');

function linuxTauriRoot({ mode = null } = {}) {
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

describe('Linux WebKitGTK compatibility', () => {
    it('bridges custom-scheme audio without refetching the same source', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot();

        expect(installMediaCompatibility(root)).toBe('active');
        const audio = new FakeMediaElement('tauri://localhost/audio/rew.mp3', 'AUDIO');

        await expect(audio.play()).resolves.toBe('played');
        expect(root.fetch).toHaveBeenCalledWith('tauri://localhost/audio/rew.mp3');
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(audio.src).toBe('blob:deltamod-media-1');
        expect(root.DeltamodLinuxCompat.snapshot().audioBlobLoads).toBe(1);

        await audio.play();
        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(nativePlay).toHaveBeenCalledTimes(2);
    });

    it('revokes stale object URLs when an audio element changes source', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('tauri://localhost/audio/rew.mp3', 'AUDIO');

        await audio.play();
        audio.src = 'themeprot://asset/ch5.mp3';
        await audio.play();

        expect(root.fetch).toHaveBeenNthCalledWith(1, 'tauri://localhost/audio/rew.mp3');
        expect(root.fetch).toHaveBeenNthCalledWith(2, 'themeprot://asset/ch5.mp3');
        expect(CompatibleURL.revokeObjectURL).toHaveBeenCalledWith('blob:deltamod-media-1');
        expect(audio.dataset.deltamodOriginalMediaSource).toBe('themeprot://asset/ch5.mp3');
    });

    it('releases one-shot media blobs after playback ends', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot();
        installMediaCompatibility(root);
        const audio = new FakeMediaElement('tauri://localhost/audio/rew.mp3', 'AUDIO');

        await audio.play();
        audio.emit('ended');

        expect(CompatibleURL.revokeObjectURL).toHaveBeenCalledWith('blob:deltamod-media-1');
        expect(root.DeltamodLinuxCompat.snapshot().revokedObjectUrls).toBe(1);
    });

    it('keeps video on native streaming in auto mode so failures can use the poster fallback', async () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        installMediaCompatibility(root);
        const video = new FakeMediaElement(
            'tauri://localhost/themes/video/chara-theme.mp4',
            'VIDEO'
        );

        await expect(video.play()).resolves.toBe('played');
        expect(root.fetch).not.toHaveBeenCalled();
        expect(nativePlay).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxCompat.snapshot().videoDirectPlays).toBe(1);
    });

    it('blocks the theme background video in performance mode', async () => {
        const { root, FakeMediaElement } = linuxTauriRoot({ mode: 'performance' });
        installMediaCompatibility(root);
        const video = new FakeMediaElement(
            'tauri://localhost/themes/video/chara-theme.mp4',
            'VIDEO'
        );
        video.id = 'theme-background-video';

        await expect(video.play()).rejects.toMatchObject({
            name: 'NotSupportedError',
            code: 'DELTAMOD_LINUX_PERFORMANCE_VIDEO_DISABLED'
        });
        expect(root.fetch).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxCompat.snapshot().videoBlocks).toBe(1);
    });

    it('only Blob-bridges video when quality mode is explicitly selected', async () => {
        const { root, FakeMediaElement, CompatibleURL } = linuxTauriRoot({ mode: 'quality' });
        installMediaCompatibility(root);
        const video = new FakeMediaElement(
            'tauri://localhost/themes/video/chara-theme.mp4',
            'VIDEO'
        );

        await expect(video.play()).resolves.toBe('played');
        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(root.DeltamodLinuxCompat.snapshot().videoBlobLoads).toBe(1);
    });

    it('persists mode and applies Linux performance classes', () => {
        const { root, classes, storage } = linuxTauriRoot();
        installMediaCompatibility(root);

        expect(classes.has('deltamod-linux-webkit')).toBe(true);
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(true);
        expect(classes.has('deltamod-linux-performance')).toBe(false);

        root.DeltamodLinuxCompat.setMode('performance');
        expect(storage.get('deltamodLinuxMediaMode')).toBe('performance');
        expect(classes.has('deltamod-linux-performance')).toBe(true);

        root.DeltamodLinuxCompat.setMode('quality');
        expect(classes.has('deltamod-linux-reduced-effects')).toBe(false);
        expect(classes.has('deltamod-linux-performance')).toBe(false);
    });

    it('does not patch media playback outside Linux Tauri', () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        root.navigator = { platform: 'MacIntel', userAgent: 'WebKit macOS' };

        expect(installMediaCompatibility(root)).toBe('inactive');
        expect(FakeMediaElement.prototype.play).toBe(nativePlay);
        expect(root.DeltamodLinuxCompat).toBeUndefined();
    });

    it('loads the compatibility layer under blob-enabled HTML and Tauri CSPs', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const tauriConfig = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'src-tauri', 'tauri.conf.json'),
            'utf8'
        ));
        const csp = tauriConfig.app.security.csp;

        expect(html).toContain("media-src 'self' blob: deltapack: themeprot: packet:");
        expect(html).toContain('<link rel="stylesheet" href="linux-webkit-compat.css">');
        expect(html).toContain('<script src="./media-compat.js"></script>');
        expect(html.indexOf('./media-compat.js')).toBeGreaterThan(html.indexOf('./tauri-adapter.js'));
        expect(csp['media-src']).toContain('blob:');
        expect(csp['connect-src']).toContain('themeprot:');
        expect(csp['connect-src']).toContain('packet:');
    });

    it('keeps body text antialiased while reducing Linux-only composition effects', () => {
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'linux-webkit-compat.css'), 'utf8');

        expect(css).toContain('html.deltamod-linux-webkit body');
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
