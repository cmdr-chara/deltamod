const fs = require('fs');
const path = require('path');
const installMediaCompatibility = require('../web/media-compat');

const projectRoot = path.join(__dirname, '..');

function linuxTauriRoot() {
    const nativePlay = vi.fn(() => Promise.resolve('played'));
    class FakeMediaElement {
        constructor(source = '') {
            this.src = source;
            this.currentSrc = '';
            this.dataset = {};
        }

        getAttribute(name) {
            return name === 'src' ? this.src : null;
        }
    }
    FakeMediaElement.prototype.play = nativePlay;

    class CompatibleURL extends URL {}
    CompatibleURL.createObjectURL = vi.fn(() => 'blob:deltamod-media');
    CompatibleURL.revokeObjectURL = vi.fn();

    return {
        nativePlay,
        root: {
            navigator: { platform: 'Linux x86_64', userAgent: 'WebKit Linux' },
            location: { href: 'tauri://localhost/index.html' },
            document: { documentElement: { classList: { add: vi.fn() } } },
            __TAURI__: { core: {} },
            HTMLMediaElement: FakeMediaElement,
            URL: CompatibleURL,
            fetch: vi.fn(() => Promise.resolve({
                ok: true,
                status: 200,
                blob: () => Promise.resolve({ type: 'audio/mpeg' })
            })),
            console: { warn: vi.fn() },
            addEventListener: vi.fn()
        },
        FakeMediaElement,
        CompatibleURL
    };
}

describe('Linux WebKitGTK compatibility', () => {
    it('fetches Tauri media and plays it from a blob URL', async () => {
        const { root, FakeMediaElement, CompatibleURL, nativePlay } = linuxTauriRoot();

        expect(installMediaCompatibility(root)).toBe('active');
        const audio = new FakeMediaElement('tauri://localhost/audio/rew.mp3');

        await expect(audio.play()).resolves.toBe('played');
        expect(root.fetch).toHaveBeenCalledWith('tauri://localhost/audio/rew.mp3');
        expect(CompatibleURL.createObjectURL).toHaveBeenCalledTimes(1);
        expect(audio.src).toBe('blob:deltamod-media');
        expect(audio.dataset.deltamodOriginalMediaSource)
            .toBe('tauri://localhost/audio/rew.mp3');
        expect(nativePlay).toHaveBeenCalledTimes(1);

        await audio.play();
        expect(root.fetch).toHaveBeenCalledTimes(1);
        expect(nativePlay).toHaveBeenCalledTimes(2);
    });

    it('does not patch media playback outside Linux Tauri', () => {
        const { root, FakeMediaElement, nativePlay } = linuxTauriRoot();
        root.navigator = { platform: 'MacIntel', userAgent: 'WebKit macOS' };

        expect(installMediaCompatibility(root)).toBe('inactive');
        expect(FakeMediaElement.prototype.play).toBe(nativePlay);
    });

    it('loads the compatibility layer under a blob-enabled media CSP', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        expect(html).toContain("media-src 'self' blob: deltapack: themeprot: packet:");
        expect(html).toContain('<link rel="stylesheet" href="linux-webkit-compat.css">');
        expect(html).toContain('<script src="./media-compat.js"></script>');
        expect(html.indexOf('./media-compat.js')).toBeGreaterThan(html.indexOf('./tauri-adapter.js'));
    });

    it('forces dark non-native selects and pixel-friendly smoothing only on Linux Tauri', () => {
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'linux-webkit-compat.css'), 'utf8');
        expect(css).toContain('html.deltamod-linux-webkit');
        expect(css).toContain('color-scheme: dark;');
        expect(css).toContain('-webkit-appearance: none;');
        expect(css).toContain('appearance: none;');
        expect(css).toContain('-webkit-font-smoothing: none;');
        expect(css).toContain('-moz-osx-font-smoothing: auto;');
    });
});
