const fs = require('fs');
const path = require('path');
const { test, expect } = require('@playwright/test');

const root = path.join(__dirname, '..', '..');
const webRoot = path.join(root, 'web');

test.use(process.platform === 'win32' ? { channel: 'msedge' } : {});

test('language changes translate the rendered page without navigation', async ({ page }) => {
    await page.route('http://deltamod.test/**', async route => {
        const url = new URL(route.request().url());
        if (url.pathname === '/') {
            await route.fulfill({
                contentType: 'text/html',
                body: `<!doctype html><html lang="en"><body>
                    <nav aria-label="Options"><button title="Options">Options</button></nav>
                    <main><h1 data-i18n="options">Options</h1><p id="static-copy">Data</p></main>
                </body></html>`
            });
            return;
        }

        const match = url.pathname.match(/^\/langs\/([a-z-]+)\/(language\.json|metadata\.txt)$/);
        if (!match) {
            await route.abort();
            return;
        }
        await route.fulfill({
            contentType: match[2].endsWith('.json') ? 'application/json' : 'text/plain',
            body: fs.readFileSync(path.join(webRoot, 'langs', match[1], match[2]), 'utf8')
        });
    });

    await page.goto('http://deltamod.test/');
    await page.addScriptTag({ path: path.join(webRoot, 'modules', 'localization.js') });
    await page.evaluate(() => {
        window.__pageCalls = 0;
        window.pageN = 'main';
        window.page = async () => { window.__pageCalls += 1; };
    });
    await page.evaluate(() => window.Localization.ready);

    await page.evaluate(() => {
        const userContent = document.createElement('p');
        userContent.id = 'dynamic-user-content';
        userContent.textContent = 'Options';
        document.querySelector('main').appendChild(userContent);
    });

    for (const [code, options, data] of [
        ['it', 'Opzioni', 'Dati'],
        ['pl', 'Ustawienia', 'Dane'],
        ['es', 'Opciones', 'Datos'],
        ['fr', 'Options', 'Données'],
        ['de', 'Optionen', 'Daten'],
        ['pt-br', 'Opções', 'Dados'],
        ['ja', 'オプション', 'データ']
    ]) {
        await page.evaluate(language => window.Localization.setLanguage(language), code);
        await expect(page.locator('main h1')).toHaveText(options);
        await expect(page.locator('nav button')).toHaveText(options);
        await expect(page.locator('nav button')).toHaveAttribute('title', options);
        await expect(page.locator('#static-copy')).toHaveText(data);
        await expect(page.locator('#dynamic-user-content')).toHaveText('Options');
    }
    expect(await page.evaluate(() => window.__pageCalls)).toBe(0);
});
