const fs = require('node:fs');
const path = require('node:path');

const {
    EVENTS,
    PIXEL_PATTERNS,
    getEventForDate,
    getWesternEasterDate,
    normalizeMode
} = require('../web/modules/seasonal-events');

const projectRoot = path.join(__dirname, '..');
const localDate = (year, month, day) => new Date(year, month - 1, day, 12);

describe('universal seasonal events', () => {
    it('computes Gregorian Easter and keeps its display to Easter week', () => {
        const easter = getWesternEasterDate(2026);
        expect([easter.getFullYear(), easter.getMonth() + 1, easter.getDate()])
            .toEqual([2026, 4, 5]);
        expect(getEventForDate(localDate(2026, 3, 29))?.id).toBe('easter');
        expect(getEventForDate(localDate(2026, 4, 6))?.id).toBe('easter');
        expect(getEventForDate(localDate(2026, 4, 7))).toBeNull();
    });

    it.each([
        [localDate(2026, 5, 1), 'womens-health'],
        [localDate(2026, 5, 31), 'womens-health'],
        [localDate(2026, 6, 1), 'mens-health'],
        [localDate(2026, 6, 30), 'mens-health'],
        [localDate(2026, 10, 24), 'halloween'],
        [localDate(2026, 11, 1), 'halloween'],
        [localDate(2026, 12, 1), 'christmas'],
        [localDate(2027, 1, 6), 'christmas'],
        [localDate(2026, 12, 31), 'new-year'],
        [localDate(2027, 1, 2), 'new-year']
    ])('selects the expected event for %s', (date, expected) => {
        expect(getEventForDate(date)?.id).toBe(expected);
    });

    it('returns to Christmas after the New Year override and stays idle off-season', () => {
        expect(getEventForDate(localDate(2027, 1, 3))?.id).toBe('christmas');
        expect(getEventForDate(localDate(2026, 8, 12))).toBeNull();
    });

    it('accepts automatic, off, and each event as a safe preview mode', () => {
        expect(normalizeMode('AUTO')).toBe('auto');
        expect(normalizeMode('off')).toBe('off');
        for (const event of EVENTS) expect(normalizeMode(event.id)).toBe(event.id);
        expect(normalizeMode('not-an-event')).toBe('auto');
    });

    it('uses grid-only marks and inherits both colors from the active theme', () => {
        for (const event of EVENTS) {
            const pattern = PIXEL_PATTERNS[event.id];
            expect(pattern.length).toBeGreaterThanOrEqual(9);
            expect(pattern.every(row => /^[.#]+$/.test(row))).toBe(true);
            expect(pattern.every(row => row.length === pattern[0].length)).toBe(true);
        }

        const styles = fs.readFileSync(
            path.join(projectRoot, 'web', 'modules', 'seasonal-events.css'),
            'utf8'
        );
        expect(styles).toContain('--season-accent: var(--theme-color');
        expect(styles).toContain('--season-soul: var(--theme-soul-color');
        expect(styles).not.toMatch(/mix-blend-mode|filter:\s*blur|linear-gradient|radial-gradient/);
        expect(styles).toContain('@media (prefers-reduced-motion: reduce)');
    });

    it('keeps the two health events recognizable without relying on color', () => {
        const womensHealth = PIXEL_PATTERNS['womens-health'];
        const mensHealth = PIXEL_PATTERNS['mens-health'];

        expect(womensHealth).not.toEqual(mensHealth);
        expect(womensHealth.join('')).not.toBe(mensHealth.join(''));
        expect(womensHealth[8]).toContain('#######');
        expect(mensHealth[0]).toContain('####');
    });

    it('replaces the shell soul with the active event glyph', () => {
        const renderer = fs.readFileSync(
            path.join(projectRoot, 'web', 'modules', 'seasonal-events.js'),
            'utf8'
        );
        const styles = fs.readFileSync(
            path.join(projectRoot, 'web', 'modules', 'seasonal-events.css'),
            'utf8'
        );

        expect(renderer).toContain("document.querySelector?.('.dmodicon')");
        expect(renderer).toContain("className = 'seasonal-dmodicon-glyph'");
        expect(renderer).toContain("classList.toggle('dmodicon-seasonal-active', isActive)");
        expect(styles).toContain('.dmodicon-seasonal-active .dmodicon-soul');
        expect(styles).toContain('.dmodicon-seasonal-active .seasonal-dmodicon-glyph');
    });

    it('loads before the boot renderer and waits for the real theme colors', () => {
        const markup = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        expect(markup.indexOf('modules/seasonal-events.js'))
            .toBeLessThan(markup.indexOf('boot/deltamod-boot.js'));
        expect(renderer).toContain('window.SeasonalEvents?.setThemeReady(true)');
    });
});
