const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..', '..');
const themeDirectory = path.join(root, 'web', 'themes', 'data');
const themes = fs.readdirSync(themeDirectory)
    .filter(name => name.endsWith('.theme.json'))
    .sort()
    .map(name => ({
        ...JSON.parse(fs.readFileSync(path.join(themeDirectory, name), 'utf8')),
        builtIn: true
    }));

test.use(process.platform === 'win32' ? { channel: 'msedge' } : {});

test('Tauri theme catalogue renders, filters, and selects every built-in theme', async ({ page }) => {
    const customTheme = {
        id: 'custom_test',
        name: 'Custom Test',
        description: 'Imported through Tauri',
        builtIn: false,
        background: 'background.png',
        mainSong: null,
        musicTrack: 'No custom audio',
        color: 'rgb(205, 68, 81)',
        soulColor: '#FF0000',
        runtimeLayout: true
    };
    const themeCatalog = [...themes, customTheme];
    const markup = fs.readFileSync(
        path.join(root, 'web', 'views', 'themesel', 'index.html'),
        'utf8'
    ).replace(/^JSL$|^STYLESHEET\[[^\]]+\]$|^TITLE\[[^\]]+\]$|^AUDIO\[[^\]]+\]$/gm, '');

    await page.setContent(markup);
    await page.evaluate(({ themeCatalog }) => {
        window._onClosePage = [];
        window.genbtnstyles = () => {};
        window.themeRefresh = async () => {};
        window.elisten = (element, event, handler) => element.addEventListener(event, handler);
        window.__language = 'en';
        window.Localization = {
            t: (_key, fallback, visible, total) => window.__language === 'it'
                ? `${visible} di ${total} temi`
                : fallback.replace('{0}', visible).replace('{1}', total)
        };
        window.__pageCalls = 0;
        window.page = async () => { window.__pageCalls += 1; };
        window.ThemeSprites = {
            parseThemeColor: value => [
                Number.parseInt(value.slice(1, 3), 16),
                Number.parseInt(value.slice(3, 5), 16),
                Number.parseInt(value.slice(5, 7), 16)
            ],
            canonicalSoulColor: () => [0, 60, 255],
            renderAppIcon: async () => 'data:image/png;base64,preview'
        };
        window.deltamodBackend = {
            assetUrl: (scope, relativePath) => {
                if (scope === 'theme') return `themeprot://asset/${relativePath}`;
                return `http://tauri.localhost/${relativePath.replace(/^web\//, '')}`;
            },
            invoke: async (channel, args) => {
                if (channel === 'getThemes') return themeCatalog;
                if (channel === 'getTheme') return 'base';
                if (channel === 'setTheme') {
                    window.__selectedTheme = args[0];
                    return true;
                }
                if (channel === 'importTheme') {
                    window.__themeImport = args[0];
                    return { created: false, canceled: true };
                }
                throw new Error(`Unexpected channel: ${channel}`);
            }
        };
        window.communityAPI = { app: {} };
    }, { themeCatalog });
    await page.addScriptTag({
        path: path.join(root, 'web', 'modules', 'chara-encounter-session.js')
    });
    await page.addScriptTag({
        path: path.join(root, 'web', 'views', 'themesel', 'index.js')
    });

    const visibleThemes = themeCatalog.filter(theme => !theme.hiddenByDefault || theme.id === 'base');
    await expect(page.locator('#theme-filter')).toHaveAttribute('placeholder', 'Name, description, or music');
    await expect(page.locator('.theme-card')).toHaveCount(visibleThemes.length);
    await expect(page.locator('#theme-count')).toHaveText(`${visibleThemes.length} of ${visibleThemes.length} themes`);
    await page.evaluate(() => {
        window.__language = 'it';
        window.dispatchEvent(new CustomEvent('deltamod-language-change'));
    });
    await expect(page.locator('#theme-count')).toHaveText(`${visibleThemes.length} di ${visibleThemes.length} temi`);
    await expect(page.locator('.theme-card.is-current')).toHaveCount(1);
    await expect(page.locator('.theme-card.is-current h2')).toHaveText('Base Theme (Chapter 5)');
    const previewUrls = await page.locator('.theme-card-preview').evaluateAll(elements => (
        elements.map(element => element.style.backgroundImage)
    ));
    expect(previewUrls).toHaveLength(visibleThemes.length);
    expect(previewUrls.filter(url => url.includes('tauri.localhost/themes/img/'))).toHaveLength(visibleThemes.length - 1);
    expect(previewUrls.filter(url => url.includes('themeprot://asset/custom_test/background.png'))).toHaveLength(1);

    await page.locator('#theme-filter').fill('undertale');
    await expect(page.locator('.theme-card:not([hidden])')).toHaveCount(1);
    await expect(page.locator('#theme-count')).toHaveText(`1 di ${visibleThemes.length} temi`);

    await page.locator('#theme-filter').fill('');
    const selectable = page.locator('.theme-card:not(.is-current) .theme-select-button').first();
    const selectedName = await selectable.locator('xpath=ancestor::article').getAttribute('data-search');
    await selectable.click();
    const selectedId = await page.evaluate(() => window.__selectedTheme);
    expect(selectedId).toBeTruthy();
    expect(selectedName).toContain(themes.find(theme => theme.id === selectedId).name.toLocaleLowerCase());
    await expect(page.locator('.theme-card.is-current')).toHaveCount(1);
    await expect(page.locator(`.theme-card[data-theme-id="${selectedId}"]`)).toHaveClass(/is-current/);
    expect(await page.evaluate(() => window.__pageCalls)).toBe(0);

    await page.locator('#open-theme-import').click();
    await expect(page.locator('#theme-import-form')).toBeVisible();
    await expect(page.locator('#theme-import-icon-preview')).toHaveAttribute(
        'src',
        'data:image/png;base64,preview'
    );
    await page.locator('#theme-import-name').fill('Blue test');
    await page.locator('#theme-import-color').evaluate(input => {
        input.value = '#143e80';
        input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await expect(page.locator('#theme-import-color-value')).toHaveText('#143E80');
    await expect(page.locator('#theme-import-soul-value')).toHaveText('#003CFF');
    await page.locator('#create-theme').click();
    expect(await page.evaluate(() => window.__themeImport)).toMatchObject({
        name: 'Blue test',
        color: '#143E80',
        soulColor: '#003CFF'
    });
});
