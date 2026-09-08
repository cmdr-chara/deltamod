// Real accepted Mod Shop markup and scripts; only native/provider boundaries are mocked.
const { test, expect } = require('@playwright/test');
const { openView } = require('./fixtures/refinement-page');

async function openShop(page, options = {}) {
    await openView(page, 'gamebanana-browse', { width: options.width || 1100, beforeScript: async page => {
        await page.evaluate(options => {
            window.__shop = { requests: [], downloads: [], alerts: [], failPages: [], profile: 'ok', failDownload: false, ...options };
            const game = { id: 'undertale', name: 'UNDERTALE', gamebanana: { id: 1234 } };
            const invoke = window.deltamodBackend.invoke;
            window.deltamodBackend.invoke = async (channel, args) => {
                if (channel === 'getAvailableGames') return [game];
                if (channel === 'getCurrentGameInfo') return game;
                if (channel === 'validateGamebananaToken') return false;
                if (channel === 'dlmodURL') {
                    window.__shop.downloads.push(args);
                    if (window.__shop.failDownload) throw new Error('The import failed.');
                    return true;
                }
                return invoke(channel, args);
            };
            window.htmlAlert = async (title, message) => {
                window.__shop.alerts.push({ title, message });
                return 'ok';
            };
            // Deterministically trigger the real pagination observer without viewport races.
            window.IntersectionObserver = class {
                constructor(callback) { window.__intersect = () => callback([{ isIntersecting: true }]); }
                observe() {}
                disconnect() {}
            };
            window.communityAPI.modSources = { browse: async request => {
                if (request.url.includes('/TopSubs')) return { ok: true, result: { payload: [] } };
                const number = Number(new URL(request.url).searchParams.get('_nPage'));
                window.__shop.requests.push(number);
                if (window.__shop.failPages.includes(number)) throw new Error(`Page ${number} is unavailable.`);
                const payload = {
                    _aMetadata: { _bIsComplete: number >= 3 },
                    _aRecords: [{
                        _idRow: number, _sModelName: 'Mod', _sName: `Forest page ${number}`,
                        _sDescription: 'A community adventure.', _sProfileUrl: `https://gamebanana.com/mods/${number}`,
                        _sImageUrl: window.__placeholder,
                        _aSubmitter: { _idRow: 1, _sName: 'Forest team', _sAvatarUrl: window.__placeholder }
                    }]
                };
                if (window.__shop.empty) payload._aRecords = [];
                if (window.__shop.malformedPage === number) {
                    payload._aRecords.push({ ...payload._aRecords[0], _idRow: 99, _aSubmitter: null });
                }
                return { ok: true, result: { payload } };
            } };
            window.fetch = async url => {
                if (!String(url).includes('/ProfilePage')) throw new Error('Direct catalogue connection unavailable.');
                if (window.__shop.profile === 'offline') throw new Error('Profile connection unavailable.');
                if (window.__shop.profile === 'http') return new Response('Unavailable', { status: 503 });
                const count = window.__shop.profile === 'multiple' ? 2 : window.__shop.profile === 'empty' ? 0 : 1;
                return new Response(JSON.stringify({ _aFiles: Array.from({ length: count }, (_, i) => ({
                    _aModManagerIntegrations: [{ _idToolRow: 20575 }],
                    _sDownloadUrl: window.__shop.profile === 'malformed' ? undefined : `https://gamebanana.com/dl/${i + 1}`, _sFile: `Forest ${i + 1}.zip`,
                    _nFilesize: 2048, _tsDateAdded: 1700000000
                })) }), { status: 200 });
            };
        }, options);
    } });
}

test('a failed later catalogue page preserves rows and retries that same page', async ({ page }, info) => {
    await openShop(page, { failPages: [2], width: 800 });
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1']);
    await page.evaluate(() => window.currentPageStack.plusPage(1));
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1']);
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    expect(await page.evaluate(() => window.PAGE)).toBe(1);
    await page.evaluate(() => window.__intersect());
    expect(await page.evaluate(() => window.__shop.requests)).toEqual([1, 2]);
    await page.screenshot({ path: info.outputPath('shop-page-retry-800.png') });
    await page.evaluate(() => { window.__shop.failPages = []; });
    await page.getByRole('button', { name: 'Retry', exact: true }).click();
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1', 'Forest page 2']);
    expect(await page.evaluate(() => window.__shop.requests)).toEqual([1, 2, 2]);
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toHaveCount(0);
    await page.evaluate(() => window.currentPageStack.plusPage(1));
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1', 'Forest page 2', 'Forest page 3']);
});

test('an initial catalogue failure cannot silently advance past page one', async ({ page }) => {
    await openShop(page, { failPages: [1] });
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    await page.evaluate(() => window.__intersect());
    expect(await page.evaluate(() => window.__shop.requests)).toEqual([1]);
    await page.evaluate(() => { window.__shop.failPages = []; });
    await page.getByRole('button', { name: 'Retry', exact: true }).click();
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1']);
    await page.evaluate(() => window.__intersect());
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1', 'Forest page 2']);
});

for (const profile of ['offline', 'http']) {
    test(`a ${profile} file-list failure leaves the Download button retryable`, async ({ page }) => {
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await openShop(page, { profile });
        const download = page.getByRole('button', { name: 'Download Forest page 1', exact: true });
        await download.click();
        await expect.poll(() => page.evaluate(() => window.__shop.alerts.length)).toBe(1);
        await expect(download).toBeEnabled();
        expect(await page.evaluate(() => window.__shop.downloads)).toEqual([]);
        await page.evaluate(() => { window.__shop.profile = 'ok'; });
        await download.click();
        await expect.poll(() => page.evaluate(() => window.__shop.downloads.length)).toBe(1);
        expect(errors).toEqual([]);
    });
}

test('failed native import releases its button and navigation for a retry', async ({ page }) => {
    await openShop(page, { failDownload: true });
    const download = page.getByRole('button', { name: 'Download Forest page 1', exact: true });
    await download.click();
    await expect.poll(() => page.evaluate(() => window.__shop.alerts.length)).toBe(1);
    await expect(download).toBeEnabled();
    await expect(page.locator('.sidebar-button:disabled')).toHaveCount(0);
    await page.evaluate(() => { window.__shop.failDownload = false; });
    await download.click();
    await expect.poll(() => page.evaluate(() => window.__shop.downloads.length)).toBe(2);
    await expect(page.locator('#modDownloadStatusTitle')).toHaveText('Mod imported successfully');
});

test('multiple-file selection can be cancelled and a failed choice can be retried', async ({ page }) => {
    await openShop(page, { profile: 'multiple', failDownload: true });
    const download = page.getByRole('button', { name: 'Download Forest page 1', exact: true });
    await download.click();
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(download).toBeEnabled();
    await expect(download).toBeFocused();
    expect(await page.evaluate(() => window.__shop.downloads)).toEqual([]);
    await download.click();
    await page.getByRole('button', { name: /Forest 2\.zip/ }).click();
    await expect.poll(() => page.evaluate(() => window.__shop.alerts.length)).toBe(1);
    await expect(download).toBeEnabled();
    expect(await page.evaluate(() => window.__shop.downloads[0][0])).toBe('https://gamebanana.com/mmdl/2');
});

test('a malformed later page keeps earlier results and publishes no partial rows', async ({ page }) => {
    await openShop(page, { malformedPage: 2 });
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1']);
    await page.evaluate(() => window.currentPageStack.plusPage(1));
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1']);
    await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
    await page.evaluate(() => { window.__shop.malformedPage = null; });
    await page.getByRole('button', { name: 'Retry', exact: true }).click();
    await expect(page.locator('.modTitleSpan')).toHaveText(['Forest page 1', 'Forest page 2']);
});

test('empty results finish loading without fetching further pages', async ({ page }) => {
    await openShop(page, { empty: true });
    await expect(page.locator('#modsBody')).toContainText('No mods were found');
    await expect(page.locator('#modsBody')).not.toHaveAttribute('aria-busy');
    await page.evaluate(() => window.__intersect());
    expect(await page.evaluate(() => window.__shop.requests)).toEqual([1]);
});

test('overlapping file lookups release the second button while one import is running', async ({ page }) => {
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await openShop(page);
    await expect(page.locator('.modTitleSpan')).toHaveCount(1);
    await page.evaluate(() => window.currentPageStack.plusPage(1));
    await page.evaluate(() => {
        const fetch = window.fetch;
        window.__profileResolvers = [];
        window.fetch = async (...args) => {
            const response = await fetch(...args);
            return new Promise(resolve => window.__profileResolvers.push(() => resolve(response)));
        };
        const invoke = window.deltamodBackend.invoke;
        window.deltamodBackend.invoke = async (channel, args) => {
            const result = await invoke(channel, args);
            if (channel === 'dlmodURL') await new Promise(resolve => { window.__finishImport = resolve; });
            return result;
        };
    });
    const first = page.getByRole('button', { name: 'Download Forest page 1', exact: true });
    const second = page.getByRole('button', { name: 'Download Forest page 2', exact: true });
    await first.click();
    await second.click();
    await expect.poll(() => page.evaluate(() => window.__profileResolvers.length)).toBe(2);
    await page.evaluate(() => window.__profileResolvers[0]());
    await expect.poll(() => page.evaluate(() => window.__shop.downloads.length)).toBe(1);
    await page.evaluate(() => window.__profileResolvers[1]());
    await expect(second).toBeEnabled();
    await expect(first).toBeDisabled();
    await page.evaluate(() => window.__finishImport());
    await expect(page.locator('.sidebar-button:disabled')).toHaveCount(0);
    await second.click();
    await expect.poll(() => page.evaluate(() => window.__profileResolvers.length)).toBe(3);
    await page.evaluate(() => window.__profileResolvers[2]());
    await expect.poll(() => page.evaluate(() => window.__shop.downloads.length)).toBe(2);
    await page.evaluate(() => window.__finishImport());
    expect(errors).toEqual([]);
});

test('leaving during an import releases navigation without changing a later page lock', async ({ page }) => {
    await openShop(page);
    await page.evaluate(() => {
        const invoke = window.deltamodBackend.invoke;
        window.deltamodBackend.invoke = async (channel, args) => {
            const result = await invoke(channel, args);
            if (channel === 'dlmodURL') await new Promise(resolve => { window.__finishImport = resolve; });
            return result;
        };
        window.page = () => {
            window._onClosePage.forEach(close => close());
            window._onClosePage = [];
            window.pageN = 'main';
            document.querySelector('.viewport').replaceChildren();
        };
    });
    await page.getByRole('button', { name: 'Download Forest page 1', exact: true }).click();
    await expect.poll(() => page.evaluate(() => window.__shop.downloads.length)).toBe(1);
    await expect(page.locator('.sidebar-button:disabled')).not.toHaveCount(0);
    await page.getByRole('button', { name: 'Return home', exact: true }).click();
    await expect(page.locator('.sidebar-button:disabled')).toHaveCount(0);
    await page.locator('.sidebar-button').first().evaluate(button => { button.disabled = true; });
    await page.evaluate(() => window.__finishImport());
    await expect(page.locator('.sidebar-button').first()).toBeDisabled();
    expect(await page.evaluate(() => window.__shop.alerts)).toEqual([]);
});

for (const profile of ['empty', 'malformed']) {
    test(`an ${profile} file list offers recovery without leaving a busy download button`, async ({ page }) => {
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await openShop(page, { profile });
        const download = page.getByRole('button', { name: 'Download Forest page 1', exact: true });
        await download.click();
        await expect.poll(() => page.evaluate(() => window.__shop.alerts.length)).toBe(1);
        await expect(download).toBeEnabled();
        await expect(download).not.toHaveAttribute('aria-busy');
        expect(await page.evaluate(() => window.__shop.alerts[0].title)).toBe('One-click download not available');
        expect(await page.evaluate(() => window.__shop.downloads)).toEqual([]);
        expect(errors).toEqual([]);
    });
}
