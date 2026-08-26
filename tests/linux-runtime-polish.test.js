const installLinuxRuntimePolish = require('../web/linux-runtime-polish');

function baseRoot() {
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
            this.dataset = {};
            this.currentTime = 12;
            this.playbackRate = 1;
            this.pause = vi.fn();
            this.play = vi.fn(() => Promise.resolve());
            createdAudio.push(this);
        }
    }

    const compat = {
        isLinuxTauri: true,
        getMode: vi.fn(() => 'auto'),
        setMode: vi.fn()
    };

    return {
        compat,
        createdAudio,
        loopMenuAudio,
        menuAudio,
        root: {
            DeltamodLinuxCompat: compat,
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

describe('Linux runtime polish', () => {
    it('removes the manual menu-audio seek loop and uses native looping', () => {
        const { root, menuAudio, loopMenuAudio } = baseRoot();

        expect(installLinuxRuntimePolish(root)).toBe('active');

        expect(menuAudio.removeEventListener).toHaveBeenCalledWith('timeupdate', loopMenuAudio);
        expect(menuAudio.loop).toBe(true);
        expect(root.DeltamodLinuxRuntimePolish.snapshot().manualLoopRemoved).toBe(true);
    });

    it('reuses one rewind player instead of creating a new Audio element on every navigation', async () => {
        const { root, createdAudio } = baseRoot();
        installLinuxRuntimePolish(root);

        await root.rew();
        await root.rew();

        expect(createdAudio).toHaveLength(1);
        expect(createdAudio[0].src).toBe('audio/rew.mp3');
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

    it('falls back to the theme poster after repeated Linux WebKitGTK stalls in auto mode', async () => {
        const { root } = baseRoot();
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
        root.document.getElementById = vi.fn(id => id === 'theme-background-video' ? video : null);
        root.document.querySelector = vi.fn(selector => selector === '.bg' ? background : null);
        root.performance = { now: vi.fn(() => clock) };
        root.fallBackFromThemeVideo = vi.fn(() => Promise.resolve());

        installLinuxRuntimePolish(root);

        for (const next of [1000, 3000, 5000, 7000]) {
            clock = next;
            listeners.get('waiting')();
        }
        await Promise.resolve();

        expect(root.fallBackFromThemeVideo).toHaveBeenCalledWith(
            video,
            background,
            'repeated stalls on Linux WebKitGTK'
        );
        expect(root.DeltamodLinuxRuntimePolish.snapshot().stallFallbacks).toBe(1);
    });

    it('falls back when a native theme video drops at least 20% of sampled frames', async () => {
        const { root } = baseRoot();
        const listeners = new Map();
        let sample = { totalVideoFrames: 100, droppedVideoFrames: 0 };
        let frameCheck = null;
        const video = {
            hidden: false,
            paused: false,
            dataset: { source: 'tauri://localhost/themes/video/chara-theme.mp4' },
            src: 'tauri://localhost/themes/video/chara-theme.mp4',
            currentSrc: '',
            addEventListener: vi.fn((name, callback) => listeners.set(name, callback)),
            getVideoPlaybackQuality: vi.fn(() => sample)
        };
        const background = {};
        root.document.getElementById = vi.fn(id => id === 'theme-background-video' ? video : null);
        root.document.querySelector = vi.fn(selector => selector === '.bg' ? background : null);
        root.fallBackFromThemeVideo = vi.fn(() => Promise.resolve());
        root.setInterval = vi.fn(callback => { frameCheck = callback; return 1; });

        installLinuxRuntimePolish(root);
        sample = { totalVideoFrames: 200, droppedVideoFrames: 25 };
        frameCheck();
        await Promise.resolve();

        expect(root.fallBackFromThemeVideo).toHaveBeenCalledWith(
            video,
            background,
            'excessive dropped frames (25/100) on Linux WebKitGTK'
        );
        expect(root.DeltamodLinuxRuntimePolish.snapshot().droppedFrameFallbacks).toBe(1);
    });

    it('adds a Linux-only rendering-mode selector to the Interface options', async () => {
        const { root, compat } = baseRoot();
        let mode = 'auto';
        compat.getMode = vi.fn(() => mode);
        compat.setMode = vi.fn(value => { mode = value; return value; });

        function node(tagName) {
            return {
                tagName,
                children: [],
                dataset: {},
                style: {},
                append(...children) { this.children.push(...children); },
                appendChild(child) { this.children.push(child); return child; },
                setAttribute: vi.fn(),
                addEventListener: vi.fn(function addEventListener(name, callback) {
                    this.listeners = this.listeners || new Map();
                    this.listeners.set(name, callback);
                })
            };
        }

        const tableBody = node('tbody');
        root.document.createElement = vi.fn(tag => node(tag));
        root.document.getElementById = vi.fn(() => null);
        root.document.querySelector = vi.fn(selector => {
            if (selector === '#options') return tableBody;
            return null;
        });
        root.currentPageStack = { cat: vi.fn(() => Promise.resolve()) };

        installLinuxRuntimePolish(root);
        await root.currentPageStack.cat('ui');

        expect(tableBody.children).toHaveLength(1);
        const row = tableBody.children[0];
        const select = row.children[1].children[0].children[0];
        expect(select.id).toBe('SELECT-LINUX-PERFORMANCE-MODE');
        expect(select.children.map(option => option.value)).toEqual(['auto', 'performance', 'quality']);

        select.value = 'performance';
        select.listeners.get('change')();
        expect(compat.setMode).toHaveBeenCalledWith('performance');
    });

    it('does nothing when the Linux compatibility layer is inactive', () => {
        const { root, menuAudio } = baseRoot();
        root.DeltamodLinuxCompat = undefined;

        expect(installLinuxRuntimePolish(root)).toBe('inactive');
        expect(menuAudio.removeEventListener).not.toHaveBeenCalled();
        expect(root.DeltamodLinuxRuntimePolish).toBeUndefined();
    });
});
