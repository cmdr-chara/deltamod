const installLinuxRuntimePolish = require('../web/linux-runtime-polish');

function baseRoot({ mode = 'auto' } = {}) {
    const loopMenuAudio = vi.fn();
    const menuAudio = {
        loop: false,
        removeEventListener: vi.fn()
    };
    const createdAudio = [];

    class FakeAudio {
        constructor() {
            this.src = '';
            this.preload = '';
            const dataset = {};
            Object.defineProperty(this, 'dataset', {
                value: dataset,
                writable: false,
                configurable: false,
                enumerable: true
            });
            this.currentTime = 12;
            this.playbackRate = 1;
            this.pause = vi.fn();
            this.play = vi.fn(() => Promise.resolve());
            this.setAttribute = vi.fn((name, value) => {
                if (name === 'data-deltamod-retain-media-blob') {
                    dataset.deltamodRetainMediaBlob = String(value);
                }
            });
            createdAudio.push(this);
        }
    }

    const compat = {
        isLinuxTauri: true,
        getMode: vi.fn(() => mode),
        setMode: vi.fn(value => { mode = value; return value; })
    };

    const interpolate = (value, args) => String(value).replace(/{(\d+)}/g, (match, index) => (
        args[index] === undefined ? match : String(args[index])
    ));

    return {
        compat,
        createdAudio,
        loopMenuAudio,
        menuAudio,
        root: {
            DeltamodLinuxCompat: compat,
            Localization: {
                ready: Promise.resolve(),
                t: vi.fn((key, fallback, ...args) => interpolate(fallback, args))
            },
            audio: menuAudio,
            loopMenuAudio,
            rew: vi.fn(),
            Audio: FakeAudio,
            deltamodBackend: { invoke: vi.fn(() => Promise.resolve(true)) },
            console: { warn: vi.fn() },
            document: {
                querySelector: vi.fn(() => null),
                getElementById: vi.fn(() => null)
            },
            addEventListener: vi.fn(),
            setInterval: vi.fn(() => 1),
            clearInterval: vi.fn()
        }
    };
}

function optionDom(root) {
    function node(tagName) {
        return {
            tagName,
            children: [],
            dataset: {},
            style: {},
            hidden: false,
            append(...children) { this.children.push(...children); },
            appendChild(child) { this.children.push(child); return child; },
            setAttribute: vi.fn(function setAttribute(name, value) { this[name] = value; }),
            addEventListener: vi.fn(function addEventListener(name, callback) {
                this.listeners = this.listeners || new Map();
                this.listeners.set(name, callback);
            })
        };
    }

    const tableBody = node('tbody');
    const findById = (target, id) => {
        if (target?.id === id) return target;
        for (const child of target?.children || []) {
            const found = findById(child, id);
            if (found) return found;
        }
        return null;
    };

    root.document.createElement = vi.fn(tag => node(tag));
    root.document.getElementById = vi.fn(id => findById(tableBody, id));
    root.document.querySelector = vi.fn(selector => {
        if (selector === '#options') return tableBody;
        return null;
    });
    root.currentPageStack = { cat: vi.fn(() => Promise.resolve()) };
    return { tableBody };
}

describe('Linux runtime polish', () => {
    it('removes the manual menu-audio seek loop and uses native looping', () => {
        const { root, menuAudio, loopMenuAudio } = baseRoot();

        expect(installLinuxRuntimePolish(root)).toBe('active');

        expect(menuAudio.removeEventListener).toHaveBeenCalledWith('timeupdate', loopMenuAudio);
        expect(menuAudio.loop).toBe(true);
        expect(root.DeltamodLinuxRuntimePolish.snapshot().manualLoopRemoved).toBe(true);
    });

    it('reuses one rewind player without assigning to the read-only dataset property', async () => {
        const { root, createdAudio } = baseRoot();
        installLinuxRuntimePolish(root);

        await expect(root.rew()).resolves.toBe(true);
        await expect(root.rew()).resolves.toBe(true);

        expect(createdAudio).toHaveLength(1);
        expect(Object.getOwnPropertyDescriptor(createdAudio[0], 'dataset').writable).toBe(false);
        expect(createdAudio[0].src).toBe('audio/rew.mp3');
        expect(createdAudio[0].setAttribute).toHaveBeenCalledWith(
            'data-deltamod-retain-media-blob',
            'true'
        );
        expect(createdAudio[0].dataset.deltamodRetainMediaBlob).toBe('true');
        expect(createdAudio[0].pause).toHaveBeenCalledTimes(2);
        expect(createdAudio[0].play).toHaveBeenCalledTimes(2);
        expect(root.DeltamodLinuxRuntimePolish.snapshot().rewindPlays).toBe(2);
    });

    it('does not play managed rewind SFX when menu SFX is disabled', async () => {
        const { root, createdAudio } = baseRoot();
        root.deltamodBackend.invoke = vi.fn(() => Promise.resolve(false));
        installLinuxRuntimePolish(root);

        await expect(root.rew()).resolves.toBe(false);
        expect(createdAudio).toHaveLength(0);
    });

    it('falls back from quality video after repeated Linux WebKitGTK stalls', async () => {
        const { root } = baseRoot({ mode: 'quality' });
        const listeners = new Map();
        let clock = 0;
        const video = {
            hidden: false,
            paused: false,
            dataset: { source: 'tauri://localhost/themes/video/chara-theme.mp4' },
            src: 'tauri://localhost/themes/video/chara-theme.mp4',
            currentSrc: '',
            addEventListener: vi.fn((name, callback) => listeners.set(name, callback)),
            getVideoPlaybackQuality: vi.fn(() => ({ totalVideoFrames: 0, droppedVideoFrames: 0 }))
        };
        const background = {};
        const nativeFallback = vi.fn(() => Promise.resolve());
        root.document.getElementById = vi.fn(id => id === 'theme-background-video' ? video : null);
        root.document.querySelector = vi.fn(selector => selector === '.bg' ? background : null);
        root.performance = { now: vi.fn(() => clock) };
        root.fallBackFromThemeVideo = nativeFallback;

        installLinuxRuntimePolish(root);

        for (const next of [1000, 3000, 5000, 7000]) {
            clock = next;
            listeners.get('waiting')();
        }
        await Promise.resolve();
        await Promise.resolve();

        expect(nativeFallback).toHaveBeenCalledWith(
            video,
            background,
            'repeated stalls on Linux WebKitGTK'
        );
        expect(root.DeltamodLinuxRuntimePolish.snapshot().stallFallbacks).toBe(1);
        expect(root.DeltamodLinuxRuntimePolish.snapshot().qualityVideoFallbackReason)
            .toBe('repeated stalls on Linux WebKitGTK');
    });

    it('falls back from quality video when at least 20% of sampled frames are dropped', async () => {
        const { root } = baseRoot({ mode: 'quality' });
        let sample = { totalVideoFrames: 100, droppedVideoFrames: 0 };
        let frameCheck = null;
        const video = {
            hidden: false,
            paused: false,
            dataset: { source: 'tauri://localhost/themes/video/chara-theme.mp4' },
            src: 'tauri://localhost/themes/video/chara-theme.mp4',
            currentSrc: '',
            addEventListener: vi.fn(),
            getVideoPlaybackQuality: vi.fn(() => sample)
        };
        const background = {};
        const nativeFallback = vi.fn(() => Promise.resolve());
        root.document.getElementById = vi.fn(id => id === 'theme-background-video' ? video : null);
        root.document.querySelector = vi.fn(selector => selector === '.bg' ? background : null);
        root.fallBackFromThemeVideo = nativeFallback;
        root.setInterval = vi.fn(callback => { frameCheck = callback; return 1; });

        installLinuxRuntimePolish(root);
        sample = { totalVideoFrames: 200, droppedVideoFrames: 25 };
        frameCheck();
        await Promise.resolve();
        await Promise.resolve();

        expect(nativeFallback).toHaveBeenCalledWith(
            video,
            background,
            'excessive dropped frames (25/100) on Linux WebKitGTK'
        );
        expect(root.DeltamodLinuxRuntimePolish.snapshot().droppedFrameFallbacks).toBe(1);
        expect(root.DeltamodLinuxRuntimePolish.snapshot().qualityVideoFallbackReason)
            .toBe('excessive dropped frames (25/100) on Linux WebKitGTK');
    });

    it('does not run the quality health fallback while auto poster mode is selected', async () => {
        const { root } = baseRoot({ mode: 'auto' });
        const listeners = new Map();
        const video = {
            hidden: false,
            paused: false,
            dataset: { source: 'tauri://localhost/themes/video/chara-theme.mp4' },
            src: 'tauri://localhost/themes/video/chara-theme.mp4',
            currentSrc: '',
            addEventListener: vi.fn((name, callback) => listeners.set(name, callback)),
            getVideoPlaybackQuality: vi.fn(() => ({ totalVideoFrames: 100, droppedVideoFrames: 100 }))
        };
        root.document.getElementById = vi.fn(id => id === 'theme-background-video' ? video : null);
        root.fallBackFromThemeVideo = vi.fn(() => Promise.resolve());

        installLinuxRuntimePolish(root);
        listeners.get('waiting')();
        await Promise.resolve();

        expect(root.DeltamodLinuxRuntimePolish.snapshot().videoStalls).toBe(0);
    });

    it('adds a localized Linux-only rendering-mode selector to the Interface options', async () => {
        const { root, compat } = baseRoot();
        const translations = {
            linux_rendering_mode: 'Linux graphics mode',
            linux_rendering_mode_aria: 'Linux graphics mode',
            linux_rendering_auto: 'Automatic',
            linux_rendering_auto_desc: 'Automatic poster mode',
            linux_rendering_performance: 'Fast',
            linux_rendering_performance_desc: 'Reduced effects',
            linux_rendering_quality: 'Full quality',
            linux_rendering_quality_desc: 'Native video'
        };
        root.Localization.t = vi.fn((key, fallback, ...args) => {
            const value = translations[key] || fallback;
            return String(value).replace(/{(\d+)}/g, (match, index) => (
                args[index] === undefined ? match : String(args[index])
            ));
        });
        const { tableBody } = optionDom(root);

        installLinuxRuntimePolish(root);
        await root.currentPageStack.cat('ui');

        expect(tableBody.children).toHaveLength(1);
        const row = tableBody.children[0];
        const title = row.children[0].children[0];
        const description = row.children[0].children[2];
        const select = row.children[1].children[0].children[0];
        expect(title.textContent).toBe('Linux graphics mode');
        expect(description.textContent).toBe('Automatic poster mode');
        expect(select.id).toBe('SELECT-LINUX-PERFORMANCE-MODE');
        expect(select.children.map(option => option.value)).toEqual(['auto', 'performance', 'quality']);
        expect(select.children.map(option => option.textContent)).toEqual([
            'Automatic',
            'Fast',
            'Full quality'
        ]);

        select.value = 'performance';
        select.listeners.get('change')();
        expect(compat.setMode).toHaveBeenCalledWith('performance');
        expect(description.textContent).toBe('Reduced effects');
    });

    it('shows the Quality fallback reason without changing the underlying fallback result', async () => {
        const { root, compat } = baseRoot({ mode: 'quality' });
        const nativeFallback = vi.fn(() => Promise.resolve('poster-active'));
        root.fallBackFromThemeVideo = nativeFallback;
        const { tableBody } = optionDom(root);

        installLinuxRuntimePolish(root);
        await root.currentPageStack.cat('ui');
        await expect(root.fallBackFromThemeVideo({}, {}, 'codec unavailable'))
            .resolves.toBe('poster-active');

        const status = tableBody.children[0].children[0].children[3];
        expect(nativeFallback).toHaveBeenCalledTimes(1);
        expect(status.hidden).toBe(false);
        expect(status.textContent).toBe('Video fallback active for this theme: codec unavailable');
        expect(root.DeltamodLinuxRuntimePolish.snapshot().qualityVideoFallbackReason)
            .toBe('codec unavailable');

        compat.setMode('auto');
        root.DeltamodLinuxRuntimePolish.refreshModeControl();
        expect(status.hidden).toBe(true);
    });

    it('does nothing when the Linux compatibility layer is inactive', () => {
        const { root, menuAudio } = baseRoot();
        root.DeltamodLinuxCompat = undefined;

        expect(installLinuxRuntimePolish(root)).toBe('inactive');
        expect(menuAudio.removeEventListener).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxRuntimePolish).toBeUndefined();
    });
});
