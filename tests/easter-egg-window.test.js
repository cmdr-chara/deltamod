// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const {
    EasterEggWindowShaker,
    supportsNativeWindowPosition
} = require('../node/EasterEggWindow');

describe('Chara native window shake', () => {
    test('supports native movement except under Linux Wayland', () => {
        expect(supportsNativeWindowPosition('win32', {})).toBe(true);
        expect(supportsNativeWindowPosition('linux', { XDG_SESSION_TYPE: 'x11' })).toBe(true);
        expect(supportsNativeWindowPosition('linux', { XDG_SESSION_TYPE: 'wayland' })).toBe(false);
        expect(supportsNativeWindowPosition('linux', { WAYLAND_DISPLAY: 'wayland-0' })).toBe(false);
    });

    test('unmaximizes once, shakes around the origin, and restores it on stop', () => {
        let scheduled = null;
        const clearIntervalFn = vi.fn();
        const setIntervalFn = vi.fn((callback, intervalMs) => {
            scheduled = { callback, intervalMs, unref: vi.fn() };
            return scheduled;
        });
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => true),
            unmaximize: vi.fn(),
            getPosition: vi.fn(() => [320, 180]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({
            platform: 'win32',
            env: {},
            setIntervalFn,
            clearIntervalFn
        });

        expect(shaker.setPhase(window, 'slash')).toEqual({ phase: 'slash', native: true });
        expect(window.unmaximize).toHaveBeenCalledOnce();
        expect(window.setPosition).toHaveBeenLastCalledWith(310, 180);
        expect(scheduled.intervalMs).toBe(46);

        scheduled.callback();
        expect(window.setPosition).toHaveBeenLastCalledWith(328, 180);

        expect(shaker.setPhase(window, 'numbers')).toEqual({ phase: 'numbers', native: true });
        expect(window.unmaximize).toHaveBeenCalledOnce();
        expect(window.getPosition).toHaveBeenCalledOnce();
        expect(window.setPosition).toHaveBeenLastCalledWith(305, 180);
        expect(scheduled.intervalMs).toBe(38);

        expect(shaker.setPhase(window, 'stop')).toEqual({
            phase: 'stop',
            native: true,
            restored: true
        });
        expect(clearIntervalFn).toHaveBeenCalled();
        expect(window.setPosition).toHaveBeenLastCalledWith(320, 180);
    });

    test('falls back without trying to move a Wayland window', () => {
        const window = {
            isDestroyed: vi.fn(() => false),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({
            platform: 'linux',
            env: { XDG_SESSION_TYPE: 'wayland' }
        });

        expect(shaker.setPhase(window, 'numbers')).toEqual({ phase: 'numbers', native: false });
        expect(window.setPosition).not.toHaveBeenCalled();
    });

    test('rejects renderer-controlled shake modes outside the fixed sequence phases', () => {
        const shaker = new EasterEggWindowShaker({ platform: 'win32', env: {} });
        expect(() => shaker.setPhase({}, 'arbitrary')).toThrow(
            'Invalid easter-egg window shake phase.'
        );
    });

    test('does not attempt restoration after the shaken window is destroyed', () => {
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => false),
            getPosition: vi.fn(() => [80, 60]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({ platform: 'win32', env: {} });
        shaker.setPhase(window, 'slash');
        window.isDestroyed.mockReturnValue(true);

        expect(shaker.stop()).toBe(false);
        expect(window.setPosition).not.toHaveBeenLastCalledWith(80, 60);
    });

    test('fails closed when checking the shaken window throws', () => {
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => false),
            getPosition: vi.fn(() => [80, 60]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({ platform: 'win32', env: {} });
        shaker.setPhase(window, 'slash');
        window.setPosition.mockClear();
        window.isDestroyed.mockImplementation(() => {
            throw new Error('destroy probe failed');
        });

        expect(shaker.stop()).toBe(false);
        expect(window.setPosition).not.toHaveBeenCalled();
        expect(shaker.stop()).toBe(false);
    });

    test('fails closed when restoring the shaken window throws', () => {
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => false),
            getPosition: vi.fn(() => [80, 60]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({ platform: 'win32', env: {} });
        shaker.setPhase(window, 'slash');
        window.setPosition.mockImplementation(() => {
            throw new Error('restore failed');
        });

        expect(shaker.stop()).toBe(false);
        expect(window.setPosition).toHaveBeenLastCalledWith(80, 60);
        expect(shaker.stop()).toBe(false);
    });

    test('restores independently when the host timer clearer throws', () => {
        const timer = { unref: vi.fn() };
        const clearIntervalFn = vi.fn(() => {
            throw new Error('clear failed');
        });
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => false),
            getPosition: vi.fn(() => [80, 60]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({
            platform: 'win32',
            env: {},
            setIntervalFn: vi.fn(() => timer),
            clearIntervalFn
        });
        shaker.setPhase(window, 'slash');

        expect(shaker.stop()).toBe(true);
        expect(clearIntervalFn).toHaveBeenCalledOnce();
        expect(clearIntervalFn).toHaveBeenCalledWith(timer);
        expect(window.setPosition).toHaveBeenLastCalledWith(80, 60);
        expect(shaker.stop()).toBe(false);
    });

    test('clears a valid falsey timer handle before restoring', () => {
        const clearIntervalFn = vi.fn();
        const window = {
            isDestroyed: vi.fn(() => false),
            isFullScreen: vi.fn(() => false),
            isMaximized: vi.fn(() => false),
            getPosition: vi.fn(() => [80, 60]),
            setPosition: vi.fn()
        };
        const shaker = new EasterEggWindowShaker({
            platform: 'win32',
            env: {},
            setIntervalFn: vi.fn(() => 0),
            clearIntervalFn
        });
        shaker.setPhase(window, 'numbers');

        expect(shaker.stop()).toBe(true);
        expect(clearIntervalFn).toHaveBeenCalledWith(0);
        expect(window.setPosition).toHaveBeenLastCalledWith(80, 60);
    });
});
