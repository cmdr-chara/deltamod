const fs = require('node:fs');
const path = require('node:path');
const { test, expect } = require('@playwright/test');

const webRoot = path.resolve(__dirname, '../../web');
const mime = { '.html': 'text/html', '.js': 'application/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml', '.webp': 'image/webp', '.ttf': 'font/ttf' };
const pages = ['main', 'allmods', 'allmods-v2', 'options', 'installmanager', 'gamebanana-browse', 'collections', 'themesel', 'locate', 'patching', 'goc-dl', 'deleteall', 'collection-exportchoose', 'gamebanana-leave-comment', 'credits'];

test.use({ reducedMotion: 'reduce' });

async function openWorkspace(page) {
    await page.addInitScript({ path: path.join(__dirname, 'fixtures/workspace-runtime.js') });
    await page.route('http://deltamod.test/**', async route => {
        const pathname = decodeURIComponent(new URL(route.request().url()).pathname);
        // The React boot animation has separate integration tests. Use real startup
        // and renderer code with a deterministic completion boundary for view tests.
        if (pathname === '/boot/deltamod-boot.js') {
            await route.fulfill({ contentType: 'application/javascript', body: 'window.DeltamodBoot={setProgress(){},setTheme(){},fail(){},finish(){document.querySelector("#deltamod-boot-root").hidden=true;}};' });
            return;
        }
        const file = path.resolve(webRoot, '.' + (pathname === '/' ? '/index.html' : pathname));
        if (!file.startsWith(webRoot + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
            await route.fulfill({ status: 404, body: 'Not found' }); return;
        }
        let body = fs.readFileSync(file);
        if (pathname === '/themes/data/base.theme.json') {
            const theme = JSON.parse(body); theme.mainSong = null; delete theme.backgroundVideo;
            body = Buffer.from(JSON.stringify(theme));
        }
        await route.fulfill({ contentType: mime[path.extname(file)] || 'text/plain', body });
    });
    await page.route('https://gamebanana.com/**', route => route.fulfill({ contentType: 'application/json', body: '{"_aRecords":[],"_aMetadata":{"_nRecordCount":0}}' }));
    await page.goto('http://deltamod.test/');
    await expect(page.locator('html')).not.toHaveClass(/deltamod-route-pending/);
    await page.waitForFunction(() => window.pageN === 'main' && document.querySelectorAll('.modrow').length === 12);
}

for (const width of [1100, 800]) {
    test(`all workspace routes keep their controls inside a ${width}px window`, async ({ page }) => {
        await page.setViewportSize({ width, height: width === 800 ? 560 : 720 });
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await openWorkspace(page);
        for (const name of pages) {
            await page.evaluate(name => {
                window._pageArguments = { collectionId: 1, id: 1, model: 'Mod' };
                return window.page(name);
            }, name);
            await expect(page.locator('.viewport')).toHaveAttribute('data-page', name);
            await expect(page.locator('.viewport h1').first()).toBeVisible();
            await expect.poll(() => page.locator('.viewport').evaluate(element => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(1);
        }
        expect(errors).toEqual([]);
    });
}

test('search and sorting preserve mounted rows and do not reload the native catalogue', async ({ page }) => {
    await openWorkspace(page);
    await page.evaluate(() => { window.__savedRow = document.querySelector('.modrow'); window.__calls.length = 0; });
    await page.locator('#mod-search').fill('quiet footsteps');
    await expect(page.locator('.modrow:not([hidden])')).toHaveCount(3);
    await page.locator('#mod-search').press('Escape');
    await expect(page.locator('.modrow:not([hidden])')).toHaveCount(12);
    await page.locator('#sortWay').selectOption('desc');
    expect(await page.evaluate(() => window.__savedRow.isConnected)).toBe(true);
    expect(await page.evaluate(() => window.__calls.filter(call => ['getModList', 'getModListFull', 'getModState'].includes(call.channel)))).toEqual([]);
});

test('failed enable writes roll back and the dialog restores focus', async ({ page }) => {
    await openWorkspace(page);
    const toggle = page.locator('#modcheck-mod-0');
    await expect(toggle).toBeChecked();
    await page.evaluate(() => { window.__rejectToggle = true; });
    await toggle.focus();
    await toggle.press('Space');
    await expect(page.locator('[role="alertdialog"]')).toBeVisible();
    await expect(page.locator('.alertMsg')).toContainText('Fixture write rejected');
    await page.locator('.alertMsg button').click();
    await expect(toggle).toBeChecked();
    await expect(toggle).toBeEnabled();
    await expect(toggle).toBeFocused();
    await expect(page.locator('#par')).toBeEnabled();
});

test('queued dialogs settle false and zero and support a callback opening another dialog', async ({ page }) => {
    await openWorkspace(page);
    await page.evaluate(() => {
        window.__choices = [];
        htmlAlert('First', 'Choose false', [{ text: 'No', resolveWith: false }]).then(value => window.__choices.push(value));
        htmlAlert('Second', 'Choose zero', [{ text: 'Zero', resolveWith: 0 }]).then(value => window.__choices.push(value));
    });
    await expect(page.locator('#workspace-alert-title')).toHaveText('First');
    await page.locator('.alertMsg button').press('Escape');
    await expect(page.locator('#workspace-alert-title')).toHaveText('Second');
    await page.locator('.alertMsg button').click();
    await expect.poll(() => page.evaluate(() => window.__choices)).toEqual([false, 0]);
    await page.evaluate(() => {
        htmlAlert('Outer', 'Open another dialog', [{ text: 'Open', onClick: () => htmlAlert('Inner', 'Nested safely', [{ text: 'Done', resolveWith: true }]) }]).then(value => window.__nested = value);
    });
    await page.locator('.alertMsg button').click();
    await expect(page.locator('#workspace-alert-title')).toHaveText('Inner');
    await page.locator('.alertMsg button').click();
    await expect.poll(() => page.evaluate(() => window.__nested)).toBe(true);
});

test('patch output is bounded text and progress never shows a fabricated percentage', async ({ page }) => {
    await openWorkspace(page);
    await page.evaluate(() => window.page('patching'));
    await expect(page.locator('#patch-progress')).not.toHaveAttribute('value');
    await page.evaluate(() => {
        for (let i = 0; i < 250; i += 1) window.currentPageStack.gpl({ log: `line ${i} <img src=x onerror=alert(1)>`, percent: 42 });
    });
    await expect(page.locator('#gpl > div')).toHaveCount(200);
    await expect(page.locator('#gpl img')).toHaveCount(0);
    await expect(page.locator('#patch-progress')).toHaveAttribute('value', '42');
    await page.evaluate(() => window.currentPageStack.fp());
    await expect(page.locator('#patch-progress')).toHaveAttribute('value', '100');
    await expect(page.locator('#next')).toBeVisible();
});

test('the language picker traps Tab and exits without losing the trigger', async ({ page }) => {
    await openWorkspace(page);
    await page.locator('#language-wheel-toggle').click();
    await expect(page.locator('#language-wheel')).toBeVisible();
    const options = page.locator('.language-wheel-option');
    await options.last().focus();
    await page.keyboard.press('Tab');
    await expect(options.first()).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.locator('#language-wheel-toggle')).toBeFocused();
});
