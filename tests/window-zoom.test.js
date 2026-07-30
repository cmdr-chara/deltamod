// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const {
    getZoomCommand,
    getNextZoomFactor
} = require('../node/WindowZoom');

describe('window zoom shortcuts', () => {
    it('recognizes keyboard and numpad zoom commands', () => {
        expect(getZoomCommand({ type: 'keyDown', control: true, key: '=', code: 'Equal' })).toBe('in');
        expect(getZoomCommand({ type: 'keyDown', control: true, key: '+', code: 'Equal' })).toBe('in');
        expect(getZoomCommand({ type: 'keyDown', control: true, key: '+', code: 'NumpadAdd' })).toBe('in');
        expect(getZoomCommand({ type: 'keyDown', control: true, key: '-', code: 'Minus' })).toBe('out');
        expect(getZoomCommand({ type: 'keyDown', meta: true, key: '0', code: 'Digit0' })).toBe('reset');
    });

    it('does not capture unrelated, released, or AltGr-like input', () => {
        expect(getZoomCommand({ type: 'keyUp', control: true, key: '+' })).toBeNull();
        expect(getZoomCommand({ type: 'keyDown', control: true, alt: true, key: '+' })).toBeNull();
        expect(getZoomCommand({ type: 'keyDown', control: false, key: '+' })).toBeNull();
        expect(getZoomCommand({ type: 'keyDown', control: true, key: 'a' })).toBeNull();
    });

    it('steps through bounded zoom factors and resets exactly', () => {
        expect(getNextZoomFactor(1, 'in')).toBe(1.1);
        expect(getNextZoomFactor(1.1, 'in')).toBe(1.25);
        expect(getNextZoomFactor(1, 'out')).toBe(0.9);
        expect(getNextZoomFactor(2, 'in')).toBe(2);
        expect(getNextZoomFactor(0.75, 'out')).toBe(0.75);
        expect(getNextZoomFactor(1.5, 'reset')).toBe(1);
    });
});
