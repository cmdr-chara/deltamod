(function (globalScope, factory) {
    const api = factory();

    if (typeof module === 'object' && module.exports) {
        module.exports = api;
    }

    if (globalScope?.document) {
        const controller = api.createController(globalScope);
        globalScope.SeasonalEvents = Object.freeze({
            events: api.EVENTS,
            getEventForDate: api.getEventForDate,
            getWesternEasterDate: api.getWesternEasterDate,
            getMode: controller.getMode,
            getActiveEvent: controller.getActiveEvent,
            setMode: controller.setMode,
            setThemeReady: controller.setThemeReady,
            refresh: controller.refresh
        });
    }
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    'use strict';

    const STORAGE_KEY = 'deltamodSeasonalMode';
    const AUTO_MODE = 'auto';
    const OFF_MODE = 'off';
    const PIXEL_SIZE = 6;
    const SHELL_PIXEL_SIZE = 2;
    const PARTICLE_COUNT = 18;

    const EVENTS = Object.freeze([
        Object.freeze({ id: 'womens-health', labelKey: 'seasonal_womens_health' }),
        Object.freeze({ id: 'mens-health', labelKey: 'seasonal_mens_health' }),
        Object.freeze({ id: 'easter', labelKey: 'seasonal_easter' }),
        Object.freeze({ id: 'halloween', labelKey: 'seasonal_halloween' }),
        Object.freeze({ id: 'christmas', labelKey: 'seasonal_christmas' }),
        Object.freeze({ id: 'new-year', labelKey: 'seasonal_new_year' })
    ]);

    const EVENT_IDS = new Set(EVENTS.map(event => event.id));

    // Every mark is drawn on the same small integer grid. The two health
    // events use different silhouettes so they remain identifiable even when
    // both inherit the same active theme colors.
    const PIXEL_PATTERNS = Object.freeze({
        'womens-health': Object.freeze([
            '...#####...',
            '..##...##..',
            '.##.....##.',
            '.##.....##.',
            '.##.....##.',
            '..##...##..',
            '...#####...',
            '.....#.....',
            '..#######..',
            '.....#.....',
            '.....#.....'
        ]),
        'mens-health': Object.freeze([
            '........####',
            '........#..#',
            '........#.#.',
            '....#####...',
            '...##..##...',
            '..##....##..',
            '..##....##..',
            '..##....##..',
            '...##..##...',
            '....####....'
        ]),
        easter: Object.freeze([
            '....###....',
            '...#####...',
            '..##...##..',
            '.##.#.#.##.',
            '.##..#..##.',
            '.#.#...#.#.',
            '.##..#..##.',
            '..##...##..',
            '...#####...',
            '....###....'
        ]),
        halloween: Object.freeze([
            '....#......',
            '..#######..',
            '.#########.',
            '##.##.##.##',
            '##.##.##.##',
            '###########',
            '##.#...#.##',
            '.##.###.##.',
            '..#######..',
            '...#####...'
        ]),
        christmas: Object.freeze([
            '.....#.....',
            '....###....',
            '...#####...',
            '....###....',
            '..#######..',
            '.#########.',
            '...#####...',
            '..#######..',
            '.#########.',
            '....###....',
            '...#####...'
        ]),
        'new-year': Object.freeze([
            '.....#.....',
            '.....#.....',
            '..#..#..#..',
            '...#.#.#...',
            '....###....',
            '###########',
            '....###....',
            '...#.#.#...',
            '..#..#..#..',
            '.....#.....',
            '.....#.....'
        ])
    });

    function isValidDate(date) {
        return date instanceof Date && Number.isFinite(date.getTime());
    }

    function getWesternEasterDate(year) {
        if (!Number.isInteger(year) || year < 1583 || year > 4099) return null;

        const a = year % 19;
        const b = Math.floor(year / 100);
        const c = year % 100;
        const d = Math.floor(b / 4);
        const e = b % 4;
        const f = Math.floor((b + 8) / 25);
        const g = Math.floor((b - f + 1) / 3);
        const h = (19 * a + b - d - g + 15) % 30;
        const i = Math.floor(c / 4);
        const k = c % 4;
        const l = (32 + 2 * e + 2 * i - h - k) % 7;
        const m = Math.floor((a + 11 * h + 22 * l) / 451);
        const month = Math.floor((h + l - 7 * m + 114) / 31);
        const day = ((h + l - 7 * m + 114) % 31) + 1;

        return new Date(year, month - 1, day);
    }

    function localDayNumber(date) {
        return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86400000;
    }

    function eventById(id) {
        return EVENTS.find(event => event.id === id) || null;
    }

    function getEventForDate(date = new Date()) {
        if (!isValidDate(date)) return null;

        const month = date.getMonth() + 1;
        const day = date.getDate();

        // New Year has priority over the Christmas season where they overlap.
        if ((month === 12 && day === 31) || (month === 1 && day <= 2)) {
            return eventById('new-year');
        }
        if ((month === 12 && day >= 1) || (month === 1 && day <= 6)) {
            return eventById('christmas');
        }
        if ((month === 10 && day >= 24) || (month === 11 && day === 1)) {
            return eventById('halloween');
        }
        if (month === 5) return eventById('womens-health');
        if (month === 6) return eventById('mens-health');

        const easter = getWesternEasterDate(date.getFullYear());
        if (easter) {
            const daysFromEaster = localDayNumber(date) - localDayNumber(easter);
            if (daysFromEaster >= -7 && daysFromEaster <= 1) {
                return eventById('easter');
            }
        }

        return null;
    }

    function normalizeMode(value) {
        const mode = String(value || '').trim().toLowerCase();
        return mode === AUTO_MODE || mode === OFF_MODE || EVENT_IDS.has(mode)
            ? mode
            : AUTO_MODE;
    }

    function hashSeed(value) {
        let hash = 2166136261;
        for (const character of value) {
            hash ^= character.charCodeAt(0);
            hash = Math.imul(hash, 16777619);
        }
        return hash >>> 0;
    }

    function seededRandom(seed) {
        let state = seed >>> 0;
        return function random() {
            state += 0x6D2B79F5;
            let value = state;
            value = Math.imul(value ^ value >>> 15, value | 1);
            value ^= value + Math.imul(value ^ value >>> 7, value | 61);
            return ((value ^ value >>> 14) >>> 0) / 4294967296;
        };
    }

    function patternGeometry(pattern, pixelSize = PIXEL_SIZE) {
        const points = [];
        pattern.forEach((row, y) => {
            [...row].forEach((cell, x) => {
                if (cell === '#') points.push([x, y]);
            });
        });

        const first = points[0] || [0, 0];
        return {
            width: Math.max(...points.map(([x]) => x), 0) * pixelSize + pixelSize,
            height: Math.max(...points.map(([, y]) => y), 0) * pixelSize + pixelSize,
            shadow: points.slice(1).map(([x, y]) => (
                `${(x - first[0]) * pixelSize}px ${(y - first[1]) * pixelSize}px 0 currentColor`
            )).join(', '),
            offsetX: first[0] * pixelSize,
            offsetY: first[1] * pixelSize
        };
    }

    function paintPattern(mark, pattern, pixelSize) {
        const geometry = patternGeometry(pattern, pixelSize);
        mark.style.width = `${pixelSize}px`;
        mark.style.height = `${pixelSize}px`;
        mark.style.marginLeft = `${geometry.offsetX}px`;
        mark.style.marginTop = `${geometry.offsetY}px`;
        mark.style.boxShadow = geometry.shadow;
        return geometry;
    }

    function createController(scope) {
        const document = scope.document;
        const storage = scope.localStorage;
        const host = document.getElementById('deltamod-seasonal-layer') || document.createElement('div');
        const shellIcon = document.querySelector?.('.dmodicon') || null;
        let themeReady = false;
        let activeEvent = null;
        let midnightTimer = null;
        let visibilityTimer = null;
        let activationFrame = null;
        let sessionMode = null;

        if (!host.id) {
            host.id = 'deltamod-seasonal-layer';
            host.className = 'deltamod-seasonal-layer';
            host.hidden = true;
            host.setAttribute('aria-hidden', 'true');
            document.body.appendChild(host);
        }

        const primaryCorner = document.createElement('span');
        primaryCorner.className = 'seasonal-corner seasonal-corner-primary';
        const secondaryCorner = document.createElement('span');
        secondaryCorner.className = 'seasonal-corner seasonal-corner-secondary';
        const primaryMark = document.createElement('i');
        primaryMark.className = 'seasonal-pixel-mark';
        const secondaryMark = document.createElement('i');
        secondaryMark.className = 'seasonal-pixel-mark';
        const particles = document.createElement('span');
        particles.className = 'seasonal-particles';
        const shellGlyph = document.createElement('span');
        shellGlyph.className = 'seasonal-dmodicon-glyph';
        const shellMark = document.createElement('i');
        shellMark.className = 'seasonal-pixel-mark';

        primaryCorner.appendChild(primaryMark);
        secondaryCorner.appendChild(secondaryMark);
        host.replaceChildren(primaryCorner, secondaryCorner, particles);
        shellGlyph.appendChild(shellMark);
        shellIcon?.appendChild(shellGlyph);

        for (let index = 0; index < PARTICLE_COUNT; index += 1) {
            const particle = document.createElement('i');
            particle.className = 'seasonal-particle';
            particle.dataset.tone = index % 3 === 0 ? 'soul' : 'accent';
            particles.appendChild(particle);
        }

        function readMode() {
            if (sessionMode) return sessionMode;
            try {
                return normalizeMode(storage.getItem(STORAGE_KEY));
            } catch {
                return AUTO_MODE;
            }
        }

        function writeMode(mode) {
            sessionMode = mode;
            try {
                storage.setItem(STORAGE_KEY, mode);
            } catch {
                // The current session can still update if storage is unavailable.
            }
        }

        function updatePattern(event) {
            const pattern = PIXEL_PATTERNS[event.id];
            const geometry = paintPattern(primaryMark, pattern, PIXEL_SIZE);
            paintPattern(secondaryMark, pattern, PIXEL_SIZE);
            for (const mark of [primaryMark, secondaryMark]) {
                mark.dataset.event = event.id;
            }
            for (const corner of [primaryCorner, secondaryCorner]) {
                corner.style.width = `${geometry.width}px`;
                corner.style.height = `${geometry.height}px`;
            }

            const shellGeometry = paintPattern(shellMark, pattern, SHELL_PIXEL_SIZE);
            shellGlyph.style.width = `${shellGeometry.width}px`;
            shellGlyph.style.height = `${shellGeometry.height}px`;
            shellGlyph.dataset.event = event.id;
        }

        function updateParticles(event) {
            const random = seededRandom(hashSeed(event.id));
            [...particles.children].forEach((particle, index) => {
                particle.style.setProperty('--season-x', `${Math.round((4 + random() * 92) * 10) / 10}%`);
                particle.style.setProperty('--season-y', `${Math.round((8 + random() * 84) * 10) / 10}%`);
                particle.style.setProperty('--season-delay', `${Math.round(random() * -900) / 100}s`);
                particle.style.setProperty('--season-duration', `${Math.round((5.4 + random() * 5.2) * 10) / 10}s`);
                particle.style.setProperty('--season-drift', `${Math.round((random() - 0.5) * 80)}px`);
                particle.style.setProperty('--season-size', `${index % 5 === 0 ? 6 : index % 2 === 0 ? 4 : 3}px`);
            });
        }

        function apply(event) {
            activeEvent = event;
            const isActive = Boolean(themeReady && event);
            const wasActive = host.dataset.active === 'true' && !host.hidden;

            if (event) {
                host.dataset.event = event.id;
                document.documentElement.dataset.seasonalEvent = event.id;
                updatePattern(event);
                updateParticles(event);
            } else {
                delete host.dataset.event;
                delete document.documentElement.dataset.seasonalEvent;
            }
            shellIcon?.classList.toggle('dmodicon-seasonal-active', isActive);
            shellGlyph.dataset.active = isActive ? 'true' : 'false';

            if (visibilityTimer !== null) scope.clearTimeout(visibilityTimer);
            if (activationFrame !== null) {
                if (scope.cancelAnimationFrame) scope.cancelAnimationFrame(activationFrame);
                else scope.clearTimeout(activationFrame);
                activationFrame = null;
            }

            if (isActive) {
                host.hidden = false;
                if (wasActive) {
                    host.dataset.active = 'true';
                } else {
                    host.dataset.active = 'false';
                    const activate = () => {
                        activationFrame = null;
                        if (themeReady && activeEvent) host.dataset.active = 'true';
                    };
                    activationFrame = scope.requestAnimationFrame
                        ? scope.requestAnimationFrame(activate)
                        : scope.setTimeout(activate, 0);
                }
            } else {
                host.dataset.active = 'false';
                visibilityTimer = scope.setTimeout(() => {
                    visibilityTimer = null;
                    if (host.dataset.active !== 'true') host.hidden = true;
                }, 430);
            }
            scope.dispatchEvent?.(new scope.CustomEvent('deltamod-seasonal-change', {
                detail: { event: event?.id || null, active: isActive, mode: readMode() }
            }));
        }

        function selectedEvent(now = new Date()) {
            const mode = readMode();
            if (mode === OFF_MODE) return null;
            return mode === AUTO_MODE ? getEventForDate(now) : eventById(mode);
        }

        function scheduleMidnightRefresh() {
            if (midnightTimer !== null) scope.clearTimeout(midnightTimer);
            const now = new Date();
            const nextDay = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1);
            midnightTimer = scope.setTimeout(() => {
                refresh();
                scheduleMidnightRefresh();
            }, Math.max(1000, nextDay.getTime() - now.getTime()));
        }

        function refresh(now = new Date()) {
            apply(selectedEvent(now));
            return activeEvent;
        }

        function setMode(value) {
            const mode = normalizeMode(value);
            writeMode(mode);
            refresh();
            return mode;
        }

        function setThemeReady(ready = true) {
            themeReady = Boolean(ready);
            refresh();
        }

        scope.addEventListener?.('storage', event => {
            if (event.key === STORAGE_KEY) {
                sessionMode = null;
                refresh();
            }
        });
        scheduleMidnightRefresh();
        refresh();

        return Object.freeze({
            getMode: readMode,
            getActiveEvent: () => activeEvent,
            setMode,
            setThemeReady,
            refresh
        });
    }

    return Object.freeze({
        AUTO_MODE,
        OFF_MODE,
        EVENTS,
        PIXEL_PATTERNS,
        getEventForDate,
        getWesternEasterDate,
        normalizeMode,
        createController
    });
});
