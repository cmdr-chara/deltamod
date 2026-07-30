// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const SHAKE_PHASES = Object.freeze({
    slash: Object.freeze({
        intervalMs: 46,
        offsets: Object.freeze([
            Object.freeze([-10, 0]),
            Object.freeze([8, 0]),
            Object.freeze([-7, 1]),
            Object.freeze([11, -1])
        ])
    }),
    numbers: Object.freeze({
        intervalMs: 38,
        offsets: Object.freeze([
            Object.freeze([-15, 0]),
            Object.freeze([13, 1]),
            Object.freeze([-11, -1]),
            Object.freeze([16, 0]),
            Object.freeze([-8, 1]),
            Object.freeze([10, -1])
        ])
    })
});

function supportsNativeWindowPosition(platform = process.platform, env = process.env) {
    if (platform !== 'linux') return true;
    return String(env.XDG_SESSION_TYPE || '').toLowerCase() !== 'wayland'
        && !env.WAYLAND_DISPLAY;
}

class EasterEggWindowShaker {
    constructor({
        platform = process.platform,
        env = process.env,
        setIntervalFn = setInterval,
        clearIntervalFn = clearInterval
    } = {}) {
        this.platform = platform;
        this.env = env;
        this.setIntervalFn = setIntervalFn;
        this.clearIntervalFn = clearIntervalFn;
        this.state = null;
    }

    setPhase(window, phase) {
        if (phase === 'stop') {
            this.stop();
            return { phase, native: supportsNativeWindowPosition(this.platform, this.env) };
        }
        const config = SHAKE_PHASES[phase];
        if (!config) {
            const error = new Error('Invalid easter-egg window shake phase.');
            error.code = 'INVALID_EASTER_EGG_SHAKE_PHASE';
            throw error;
        }
        if (!window || window.isDestroyed?.()) {
            const error = new Error('The application window is unavailable.');
            error.code = 'EASTER_EGG_WINDOW_UNAVAILABLE';
            throw error;
        }

        if (!supportsNativeWindowPosition(this.platform, this.env)) {
            this.stop();
            return { phase, native: false };
        }

        if (!this.state || this.state.window !== window) {
            this.stop();
            if (window.isFullScreen?.()) window.setFullScreen(false);
            if (window.isMaximized?.()) window.unmaximize();
            const [x, y] = window.getPosition();
            this.state = {
                window,
                origin: [x, y],
                timer: null,
                tick: 0
            };
        } else if (this.state.timer) {
            this.clearIntervalFn(this.state.timer);
            this.state.timer = null;
        }

        const runTick = () => {
            if (!this.state || window.isDestroyed?.()) {
                this.stop();
                return;
            }
            const offset = config.offsets[this.state.tick % config.offsets.length];
            this.state.tick += 1;
            window.setPosition(
                this.state.origin[0] + offset[0],
                this.state.origin[1] + offset[1]
            );
        };

        this.state.tick = 0;
        runTick();
        this.state.timer = this.setIntervalFn(runTick, config.intervalMs);
        this.state.timer?.unref?.();
        return { phase, native: true };
    }

    stop() {
        if (this.state?.timer) this.clearIntervalFn(this.state.timer);
        this.state = null;
    }
}

module.exports = {
    EasterEggWindowShaker,
    SHAKE_PHASES,
    supportsNativeWindowPosition
};
