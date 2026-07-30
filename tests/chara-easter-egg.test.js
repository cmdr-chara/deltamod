// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const projectRoot = path.resolve(__dirname, '..');
const assetRoot = path.join(projectRoot, 'web', 'assets', 'chara-easter-egg');

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

        expect(rendererSource).toContain("window.electronAPI.invoke('getUniqueFlag', ['AUDIO'])");
        expect(rendererSource).toContain("window.electronAPI.invoke('getUniqueFlag', ['SFX'])");
        expect(rendererSource).toContain('if (musicEnabled) fallenLoop.play()');
        expect(rendererSource.match(/if \(sfxEnabled\)/g)).toHaveLength(3);
    });
});
