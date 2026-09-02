// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const projectRoot = path.resolve(__dirname, '..');
const assetRoot = path.join(projectRoot, 'web', 'assets', 'chara-easter-egg');
const { createSessionGate } = require('../web/modules/chara-encounter-session.js');

describe('Chara theme-selector encounter', () => {
    test('ships every local asset required by the sequence', () => {
        const requiredAssets = [
            'chara-normal.png',
            'chara-weird.png',
            'chara-laugh-0.png',
            'chara-laugh-1.png',
            'chara-laugh-2.png',
            ...Array.from({ length: 6 }, (_, index) => `strike-${index}.png`),
            'num9-tile.png',
            'fallen-child.ogg',
            'chara-laugh.ogg',
            'slash.wav',
            'damage.wav'
        ];

        for (const asset of requiredAssets) {
            const assetPath = path.join(assetRoot, asset);
            expect(fs.existsSync(assetPath), `${asset} should exist`).toBe(true);
            expect(fs.statSync(assetPath).size, `${asset} should not be empty`).toBeGreaterThan(100);
        }
    });

    test('uses the dedicated presentation-only quit contract', () => {
        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );
        const mainSource = fs.readFileSync(
            path.join(projectRoot, 'node', 'IPCHandlers.js'),
            'utf8'
        );
        const handlerStart = mainSource.indexOf("handle('quitCommunityForEasterEgg'");
        const handlerEnd = mainSource.indexOf("handle('sampleError'", handlerStart);
        const quitHandler = mainSource.slice(handlerStart, handlerEnd);

        expect(rendererSource).toContain('window.communityAPI.app.quitForEasterEgg()');
        expect(quitHandler).toContain('app.quit()');
        expect(quitHandler).not.toMatch(/\b(?:rm|unlink|rename|writeFile|initialize)\w*\s*\(/);
    });

    test('respects the menu audio settings during the encounter', () => {
        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );

        expect(rendererSource).toContain("window.deltamodBackend.invoke('getUniqueFlag', ['AUDIO'])");
        expect(rendererSource).toContain("window.deltamodBackend.invoke('getUniqueFlag', ['SFX'])");
        expect(rendererSource).toContain('if (musicEnabled) fallenLoop.play()');
        expect(rendererSource.match(/if \(sfxEnabled\)/g)).toHaveLength(3);
    });

    test('keeps the modal keyboard-bounded and announces only completed dialogue', () => {
        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );

        expect(rendererSource).not.toContain("dialogue.setAttribute('aria-live'");
        expect(rendererSource).toContain("dialogueAnnouncer.setAttribute('aria-live', 'polite')");
        expect(rendererSource).toContain('dialogueAnnouncer.textContent = typingText');
        expect(rendererSource).toContain("if (event.key === 'Escape')");
        expect(rendererSource).toContain("if (event.key === 'Tab')");
        expect(rendererSource).toContain('cleanup({ refreshUnlockedTheme: true })');
        expect(rendererSource).toContain('if (!charaSessionGate.isCurrent(sessionToken)) return;');
        expect(rendererSource).toMatch(
            /const showChoices = \(\) => \{[\s\S]*?overlay\.dataset\.phase = phase;/
        );
    });

    test('scopes the true name to an encounter and persists the Chara theme unlock once', () => {
        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );

        expect(rendererSource).toContain("const CHARA_UNLOCK_FLAG = 'CHARA_THEME_UNLOCKED'");
        expect(rendererSource).toContain("invoke('setUniqueFlag', [CHARA_UNLOCK_FLAG, true])");
        expect(rendererSource).toMatch(
            /const themeFilterPlaceholder = \(\) => charaEncounterActive\s*\? 'THE TRUE NAME'\s*: t\('theme_filter_placeholder'/
        );
        expect(rendererSource).toContain('filterInput.placeholder = themeFilterPlaceholder()');
        expect(rendererSource).toContain("const isUnlockedChara = theme.id === 'chara' && charaUnlocked");
        expect(rendererSource).toContain('if (!charaUnlocked) {');
        expect(rendererSource).toContain('charaEncounterActive = false;');
        expect(rendererSource).toContain("invoke('setTheme', ['chara'])");
    });

    test('cancels a delayed startup before it can create or focus the encounter', async () => {
        const gate = createSessionGate();
        const token = gate.begin();
        let releaseStartup;
        const delayedFlags = new Promise(resolve => {
            releaseStartup = resolve;
        });
        let modalCreated = false;
        const startup = (async () => {
            await delayedFlags;
            if (!gate.isCurrent(token)) return;
            modalCreated = true;
        })();

        expect(gate.cancel(token)).toBe(true);
        releaseStartup([true, true]);
        await startup;

        expect(modalCreated).toBe(false);
        expect(gate.isCurrent(token)).toBe(false);
        expect(gate.begin()).not.toBeNull();
    });

    test('a cancelled shake completion cannot stop a replay session', async () => {
        const gate = createSessionGate();
        const firstToken = gate.begin();
        let releaseShake;
        const delayedShake = new Promise(resolve => {
            releaseShake = resolve;
        });
        let staleStopCalls = 0;
        const staleCompletion = delayedShake.then(() => {
            if (!gate.isCurrent(firstToken)) return;
            staleStopCalls += 1;
        });

        expect(gate.cancel(firstToken)).toBe(true);
        const replayToken = gate.begin();
        releaseShake({ native: true });
        await staleCompletion;

        expect(staleStopCalls).toBe(0);
        expect(gate.isCurrent(replayToken)).toBe(true);

        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );
        expect(rendererSource).toMatch(
            /\.then\(result => \{\s*if \(!charaSessionGate\.isCurrent\(sessionToken\)\) return;/
        );
    });

    test('honors reduced motion across renderer scripting and presentation CSS', () => {
        const rendererSource = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'index.js'),
            'utf8'
        );
        const css = fs.readFileSync(
            path.join(projectRoot, 'web', 'views', 'themesel', 'themesel.css'),
            'utf8'
        );
        const reducedMotionCss = css.match(
            /@media \(prefers-reduced-motion: reduce\) \{([\s\S]*)\}\s*$/
        )?.[1] || '';

        expect(rendererSource).toContain("matchMedia?.('(prefers-reduced-motion: reduce)')");
        expect(rendererSource).toContain("if (prefersReducedMotion && phase !== 'stop')");
        expect(rendererSource).toContain('if (!prefersReducedMotion) overlay.classList.add');
        expect(reducedMotionCss).toContain('.chara-easter-egg.is-window-shake-fallback');
        expect(reducedMotionCss).toContain('animation: none;');
        expect(reducedMotionCss).toMatch(
            /\.chara-easter-egg\.is-red-flashing::before\s*\{\s*display: none;/
        );
    });
});
