// Copyright © 2026 cmdr-chara. Licensed under the EUPL 1.2.

const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..', '..');
const themeMarkup = fs.readFileSync(
    path.join(root, 'web', 'views', 'themesel', 'index.html'),
    'utf8'
).replace(/^JSL$|^STYLESHEET\[[^\]]+\]$|^TITLE\[[^\]]+\]$|^AUDIO\[[^\]]+\]$/gm, '');
const themeScript = fs.readFileSync(
    path.join(root, 'web', 'views', 'themesel', 'index.js'),
    'utf8'
);
const themes = fs.readdirSync(path.join(root, 'web', 'themes', 'data'))
    .filter(name => name.endsWith('.theme.json'))
    .sort()
    .map(name => ({
        ...JSON.parse(fs.readFileSync(path.join(root, 'web', 'themes', 'data', name), 'utf8')),
        builtIn: true
    }));

test.use(process.platform === 'win32' ? { channel: 'msedge' } : {});

test('Chara replay waits for the refreshed theme page to finish loading', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setContent(themeMarkup);
    await page.evaluate(({ themeMarkup, themeScript, themes }) => {
        const flags = new Map([
            ['AUDIO', false],
            ['SFX', false],
            ['CHARA_THEME_UNLOCKED', false]
        ]);
        window._eventListeners = [];
        window._onClosePage = [];
        window.genbtnstyles = () => {};
        window.themeRefresh = async () => {};
        window.Localization = {
            t: (_key, fallback, ...args) => String(fallback).replace(/{(\d+)}/g, (match, index) => (
                args[index] === undefined ? match : String(args[index])
            ))
        };
        window.ThemeSprites = {
            parseThemeColor: () => [205, 68, 81],
            canonicalSoulColor: () => [255, 0, 0],
            renderAppIcon: async () => 'data:image/png;base64,preview'
        };
        window.communityAPI = {
            app: {
                quitForEasterEgg: async () => {},
                shakeForEasterEgg: async () => ({ native: false })
            }
        };
        window.deltamodBackend = {
            assetUrl: (_scope, relativePath) => `http://deltamod.test/${relativePath}`,
            invoke: async (channel, args) => {
                if (channel === 'getThemes') return themes;
                if (channel === 'getTheme') return 'base';
                if (channel === 'getUniqueFlag') return flags.get(args[0]) ?? false;
                if (channel === 'setUniqueFlag') {
                    flags.set(args[0], args[1]);
                    return true;
                }
                if (channel === 'setTheme') return true;
                if (channel === 'importTheme') return { created: false, canceled: true };
                throw new Error(`Unexpected channel: ${channel}`);
            }
        };
        window.elisten = (element, event, handler) => {
            element.addEventListener(event, handler);
            window._eventListeners.push({ element, event, handler });
        };
        window.page = async name => {
            if (name !== 'themesel') return true;
            for (const { element, event, handler } of window._eventListeners) {
                element.removeEventListener(event, handler);
            }
            window._eventListeners = [];
            const closeCallbacks = window._onClosePage;
            window._onClosePage = [];
            closeCallbacks.forEach(callback => callback());
            document.body.innerHTML = themeMarkup;

            // Model the gap between markup insertion and page-script readiness that
            // exposed the focus race on the macOS ARM runner.
            await new Promise(resolve => setTimeout(resolve, 75));
            (0, eval)(themeScript);
            return true;
        };
    }, { themeMarkup, themeScript, themes });
    await page.addScriptTag({
        path: path.join(root, 'web', 'modules', 'chara-encounter-session.js')
    });
    await page.addScriptTag({
        path: path.join(root, 'web', 'views', 'themesel', 'index.js')
    });

    const themeFilter = page.locator('#theme-filter');
    await expect(themeFilter).toBeVisible();
    await themeFilter.pressSequentially('chara');
    const encounter = page.locator('#chara-easter-egg');
    await expect(encounter).toBeVisible();

    await encounter.press('Escape');
    await expect(encounter).toHaveCount(0);
    await expect(themeFilter).toBeFocused();
    await expect(page.locator('.theme-card[data-theme-id="chara"]')).toBeVisible();

    await themeFilter.pressSequentially('chara');
    await expect(page.locator('#chara-easter-egg')).toBeVisible();
});
