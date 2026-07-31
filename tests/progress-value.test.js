// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-30.
// Licensed under the EUPL 1.2.

const { normalizeProgressFraction } = require('../node/progress/ProgressValue');

describe('progress values', () => {
    it('preserves finite fractions inside the supported range', () => {
        expect(normalizeProgressFraction(0)).toBe(0);
        expect(normalizeProgressFraction(0.42)).toBe(0.42);
        expect(normalizeProgressFraction(1)).toBe(1);
    });

    it('clamps values that would overflow the progress UI', () => {
        expect(normalizeProgressFraction(-0.1)).toBe(0);
        expect(normalizeProgressFraction(1.5)).toBe(1);
    });

    it('turns invalid progress into a safe initial value', () => {
        expect(normalizeProgressFraction(undefined)).toBe(0);
        expect(normalizeProgressFraction(Number.NaN)).toBe(0);
        expect(normalizeProgressFraction(Number.POSITIVE_INFINITY)).toBe(0);
    });
});
