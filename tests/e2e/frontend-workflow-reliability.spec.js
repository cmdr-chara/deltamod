const { test, expect } = require('@playwright/test');
const path = require('node:path');
const { openView, read, web } = require('./fixtures/refinement-page');

async function deferCommands(page, channels) {
    await page.evaluate(channels => {
        const invoke = window.deltamodBackend.invoke;
        window.__deferred = [];
        window.__alerts = [];
        window.htmlAlert = async (...args) => { window.__alerts.push(args); return 'ok'; };
        window.deltamodBackend.invoke = (channel, args = []) => {
            if (!channels.includes(channel)) return invoke(channel, args);
            window.__calls.push([channel, args]);
            return new Promise((resolve, reject) => {
                window.__deferred.push({ channel, args, resolve, reject });
            });
        };
    }, channels);
}

async function settleCommand(page, channel, { reject = false, value = true } = {}) {
    await page.evaluate(({ channel, reject, value }) => {
        const index = window.__deferred.findIndex(call => call.channel === channel);
        const call = window.__deferred.splice(index, 1)[0];
        if (!call) throw new Error(`No pending ${channel} command`);
        if (reject) call.reject(new Error('Simulated configuration write failure'));
        else call.resolve(value);
    }, { channel, reject, value });
}

async function configureVariants(page, saved = true) {
    await page.evaluate(saved => {
        window.__mods[0].variants = [
            { filename: 'normal.patch', name: 'Normal' },
            { filename: 'alternate.patch', name: 'Alternate' }
        ];
        if (saved) window.__mods[0]._selectedVariant = 'normal.patch';
        window.__states['mod-0'] = true;
    }, saved);
}

// The shell disposes the old page before replacing its markup, then loads the
// view script. Preserve pending native calls to exercise that same lifecycle.
async function returnToHome(page) {
    await page.evaluate(markup => {
        window._onClosePage.splice(0).forEach(dispose => dispose());
        window.pageN = 'options';
        document.querySelector('.viewport').innerHTML = '<div>Options</div>';
        window.pageN = 'main';
        window.currentPageStack = {};
        window._pageArguments = {};
        const body = markup.replace(/^(?:JSL|NO-SIDEBAR|(?:STYLESHEET|TITLE|AUDIO|THEME-AUDIO-EXCLUDE)\[[^\]]*\])\s*$/gm, '');
        document.querySelector('.viewport').innerHTML = window.Localization.replaceTokens(body);
    }, read('views/main/index.html'));
    await page.addScriptTag({ path: path.join(web, 'views/main/index.js') });
}

test('Home ignores a stale catalogue after navigating away and returning', async ({ page }) => {
    await openView(page, 'main', {
        count: 1,
        beforeScript: page => deferCommands(page, ['getModList'])
    });
    await returnToHome(page);
    await page.evaluate(() => {
        const current = window.__deferred[1];
        current.resolve({ modList: [{ ...window.__mods[0], name: 'Current result' }], errors: [] });
        window.__deferred.splice(1, 1);
    });
    await expect(page.locator('#par')).toBeEnabled();
    await expect(page.locator('.patch-mod-title')).toHaveText(['Current result']);

    await settleCommand(page, 'getModList', {
        value: { modList: [{
            uid: 'mod-0', folder: 'mod-0', name: 'Stale result', author: ['Author'],
            game: 'undertale', gamebanana: { supports: false }, mergeSupport: true
        }], errors: [] }
    });
    await expect(page.locator('.patch-mod-title')).toHaveText(['Current result']);
    await expect(page.locator('#modcheck-mod-0')).toHaveCount(1);
    await page.locator('#mod-search').fill('absent');
    await expect(page.locator('.modrow:visible')).toHaveCount(0);
});

test('variant writes keep launch guarded until every configuration write finishes', async ({ page }) => {
    await openView(page, 'main', {
        count: 2,
        beforeScript: async page => {
            await configureVariants(page);
            await deferCommands(page, ['setModVariant', 'toggleModState']);
        }
    });
    await expect(page.locator('#par')).toBeEnabled();
    const variant = page.locator('.patch-mod-variant');
    await variant.selectOption('alternate.patch');
    await expect(variant).toBeDisabled();
    await expect(variant).toHaveAttribute('aria-busy', 'true');
    await expect(page.locator('.control-save-status')).toHaveText('Saving…');
    await expect(page.locator('#par')).toBeDisabled();
    await page.evaluate(() => window.currentPageStack.patchAndRun());
    expect(await page.evaluate(() => window.__calls.some(([channel]) => channel === 'patchAndRun'))).toBe(false);

    await page.locator('label.patch-toggle').nth(1).click();
    await settleCommand(page, 'setModVariant');
    await expect(variant).toBeEnabled();
    await expect(page.locator('.control-save-status')).toHaveText('Saved');
    await expect(page.locator('#par')).toBeDisabled();
    await settleCommand(page, 'toggleModState');
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('#par').click();
    expect(await page.evaluate(() => window.__calls.filter(([channel]) => channel === 'patchAndRun')))
        .toEqual([['patchAndRun', [['mod-0', 'mod-1']]]]);
});

test('failed variant writes restore the saved choice and allow a successful retry', async ({ page }) => {
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await openView(page, 'main', {
        count: 1,
        beforeScript: async page => {
            await configureVariants(page);
            await deferCommands(page, ['setModVariant']);
        }
    });
    const variant = page.locator('.patch-mod-variant');
    await expect(page.locator('#par')).toBeEnabled();
    await variant.selectOption('alternate.patch');
    await settleCommand(page, 'setModVariant', { reject: true });
    await expect(variant).toHaveValue('normal.patch');
    await expect(variant).toBeEnabled();
    await expect(variant).toHaveAttribute('aria-invalid', 'true');
    await expect(page.locator('.control-save-status')).toHaveText('Not saved. Try again.');
    await expect(page.locator('#par')).toBeEnabled();

    await variant.selectOption('alternate.patch');
    await settleCommand(page, 'setModVariant');
    await expect(variant).toHaveValue('alternate.patch');
    await expect(variant).not.toHaveAttribute('aria-invalid');
    await expect(page.locator('.control-save-status')).toHaveText('Saved');
    await expect(page.locator('#par')).toBeEnabled();
    expect(errors).toEqual([]);
});

test('an unsaved initial variant cannot launch until its default choice is persisted', async ({ page }) => {
    await openView(page, 'main', {
        count: 1,
        beforeScript: async page => {
            await configureVariants(page, false);
            await deferCommands(page, ['setModVariant']);
        }
    });
    const variant = page.locator('.patch-mod-variant');
    await expect(variant).toBeVisible();
    await expect(variant).toBeDisabled();
    await expect(page.locator('#par')).toBeDisabled();
    await settleCommand(page, 'setModVariant', { reject: true });
    await expect(variant).toHaveValue('');
    await expect(variant).toBeEnabled();
    await expect(page.locator('.control-save-status')).toHaveText('Not saved. Try again.');
    await expect(page.locator('#par')).toBeDisabled();

    await variant.selectOption('normal.patch');
    await settleCommand(page, 'setModVariant');
    await expect(variant).toHaveValue('normal.patch');
    await expect(page.locator('#par')).toBeEnabled();
});

test('returning Home waits for configuration writes from the previous visit', async ({ page }) => {
    await openView(page, 'main', {
        count: 1,
        beforeScript: async page => {
            await configureVariants(page);
            await deferCommands(page, ['setModVariant']);
        }
    });
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('.patch-mod-variant').selectOption('alternate.patch');
    await returnToHome(page);
    await expect(page.locator('#par')).toBeDisabled();
    expect(await page.evaluate(() => window.__calls.filter(([channel]) => channel === 'getModList').length)).toBe(1);
    await page.evaluate(() => { window.__mods[0]._selectedVariant = 'alternate.patch'; });
    await settleCommand(page, 'setModVariant');
    await expect(page.locator('#par')).toBeEnabled();
    await expect(page.locator('.patch-mod-variant')).toHaveValue('alternate.patch');
});

test('an unsaved variant only blocks launch while its mod is enabled', async ({ page }) => {
    await openView(page, 'main', {
        count: 1,
        beforeScript: async page => {
            await configureVariants(page, false);
            await page.evaluate(() => { window.__states['mod-0'] = false; });
            await deferCommands(page, ['setModVariant']);
        }
    });
    await expect(page.locator('.patch-mod-variant')).toBeVisible();
    await settleCommand(page, 'setModVariant', { reject: true });
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('label.patch-toggle').click();
    await expect(page.locator('#par')).toBeDisabled();
    await page.locator('label.patch-toggle').click();
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('#par').click();
    expect(await page.evaluate(() => window.__calls.filter(([channel]) => channel === 'startGame')))
        .toEqual([['startGame', []]]);
});

test('a failed toggle from a departed Home visit does not interrupt the new page', async ({ page }) => {
    await openView(page, 'main', {
        count: 1,
        beforeScript: page => deferCommands(page, ['toggleModState'])
    });
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('label.patch-toggle').click();
    await returnToHome(page);
    await expect(page.locator('#par')).toBeDisabled();
    await settleCommand(page, 'toggleModState', { reject: true });
    expect(await page.evaluate(() => window.__alerts)).toEqual([]);
    await expect(page.locator('#par')).toBeEnabled();
    await expect(page.locator('#modcheck-mod-0')).not.toBeChecked();
});
