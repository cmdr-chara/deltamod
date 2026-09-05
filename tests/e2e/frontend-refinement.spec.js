const { test, expect } = require('@playwright/test');
const { openView, read } = require('./fixtures/refinement-page');

test('search and sort keep enabled mods and never refetch the list', async ({page}) => {
    await openView(page, 'main');
    await expect(page.locator('#modlist .modrow')).toHaveCount(3);
    await expect(page.locator('#par')).toBeEnabled();
    await page.locator('label.patch-toggle').first().click();
    await expect(page.locator('#modcheck-mod-0')).toBeChecked();
    const callsBefore = await page.evaluate(() => window.__calls.filter(([name]) => name === 'getModList').length);
    await page.locator('#mod-search').fill('community.forest1');
    await expect(page.locator('#modlist .modrow:visible')).toHaveCount(1);
    await page.locator('#sortWay').selectOption('desc');
    await page.locator('#clear-mod-search').click();
    await expect(page.locator('#modcheck-mod-0')).toBeChecked();
    await expect(page.locator('#modlist .patch-mod-title').first()).toHaveText('Forest 3');
    expect(await page.evaluate(() => window.__calls.filter(([name]) => name === 'getModList').length)).toBe(callsBefore);
    await page.locator('#mod-search').fill('absent');
    await expect(page.locator('.mod-search-empty')).toBeVisible();
    await page.locator('#mod-search').press('Escape');
    await expect(page.locator('#modlist .modrow:visible')).toHaveCount(3);
    await page.locator('#sortWay').selectOption('author');
    await expect(page.locator('.mod-author-heading')).toHaveCount(1);
});

test('library game filtering reuses rows and caches per-game names', async ({page}) => {
    await openView(page, 'allmods', {count:12, width:800});
    await expect(page.locator('.library-mod-row')).toHaveCount(12);
    await page.locator('#gamesShow').selectOption('undertale');
    await expect(page.locator('.library-mod-row:visible')).toHaveCount(6);
    await page.locator('#gamesShow').selectOption('all');
    await expect(page.locator('.library-mod-row:visible')).toHaveCount(12);
    expect(await page.evaluate(() => window.__calls.filter(([name]) => name === 'getGameInfo').length)).toBe(2);
    expect(await page.evaluate(() => window.__calls.filter(([name]) => name === 'getModList').length)).toBe(1);
});

test('offscreen artwork is deferred instead of scanning the entire library', async ({page}) => {
    await openView(page, 'allmods', {count:120});
    await expect(page.locator('.library-mod-row')).toHaveCount(120);
    const count = await page.evaluate(() => window.__calls.filter(([name]) => name === 'getModImage').length);
    expect(count).toBeGreaterThan(0);
    expect(count).toBeLessThan(20);
});

test('installation renaming preserves text and rolls back failed writes', async ({page}) => {
    await openView(page, 'installmanager');
    const name = page.locator('.installation-copy input');
    await expect(name).toHaveValue('My & game');
    await name.fill('Discarded');
    await name.press('Escape');
    await expect(name).toHaveValue('My & game');
    expect(await page.evaluate(() => window.__calls.filter(([n]) => n === 'setInstallationCName').length)).toBe(0);
    await page.evaluate(() => { window.__failSave = true; });
    await name.fill('Not saved');
    await name.press('Enter');
    await expect(name).toHaveValue('My & game');
    await expect(page.locator('.control-save-status')).toHaveText('Not saved. Try again.');
    await page.evaluate(() => { window.__failSave = false; });
    await name.fill('My new game');
    await name.press('Enter');
    await expect(page.locator('.control-save-status')).toHaveText('Saved');
});

test('settings show save failure without keeping an incorrect toggle value', async ({page}) => {
    await openView(page,'options');
    const flag = page.locator('#FLAG-HASHCHECKS');
    await expect(flag).toBeVisible();
    await page.evaluate(() => { window.__failSave = true; });
    await flag.click();
    await expect(flag).not.toBeChecked();
    await expect(page.locator('.control-save-status')).toHaveText('Not saved. Try again.');
    await page.evaluate(() => { window.__failSave = false; });
    await flag.click();
    await expect(flag).toBeChecked();
    await expect(page.locator('.control-save-status')).toHaveText('Saved');
});

test('themed alert queue handles rejection and falsy responses and restores focus', async ({page}) => {
    await openView(page,'main');
    const source = read('index.js');
    const alerts = source.slice(source.indexOf('let alertQueue'), source.indexOf('\nfunction credits('));
    await page.addScriptTag({content: `const MOTION={standard:0,easeIn:'ease',easeOut:'ease'};const prefersReducedMotion=()=>true;${alerts}`});
    await page.locator('#mod-search').focus();
    await page.evaluate(() => {
        window.__results = [];
        htmlAlert('First', 'Cancel safely', [{text:'Accept',resolveWith:'yes'}, {text:'Cancel',rejectWith:'cancel'}], 'warning').catch(v=>window.__results.push(v));
        htmlAlert('Second','Falsy result', [{text:'Zero',resolveWith:0}], 'check').then(v=>window.__results.push(v));
    });
    await expect(page.getByRole('alertdialog')).toBeVisible();
    await expect(page.getByRole('button',{name:'Cancel',exact:true})).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.locator('#community-alert-title')).toHaveText('Second');
    await page.getByRole('button',{name:'Zero',exact:true}).click();
    await expect(page.locator('.alertMain')).toBeHidden();
    expect(await page.evaluate(() => window.__results)).toEqual(['cancel',0]);
    await expect(page.locator('#mod-search')).toBeFocused();
    expect(await page.locator('.viewport').evaluate(node=>node.inert)).toBe(false);
});

test('patch logs are bounded text and progress can be indeterminate', async ({page}) => {
    await openView(page,'patching');
    await page.evaluate(() => {
        for(let i=0;i<450;i++) window.currentPageStack.gpl({log:`<b>Line ${i}</b>`,percent:42});
    });
    await expect(page.locator('#gpl > div')).toHaveCount(300);
    await expect(page.locator('#gpl b')).toHaveCount(0);
    await expect(page.locator('#patch-percent')).toHaveText('42%');
    await page.evaluate(() => window.currentPageStack.gpl({log:'',percent:-1}));
    expect(await page.locator('#patch-progress').getAttribute('value')).toBeNull();
    await page.evaluate(() => window.currentPageStack.fp());
    await expect(page.locator('#patch-percent')).toHaveText('100%');
    await expect(page.locator('#next')).toBeVisible();
});

test('original screens remain usable at narrow and wide desktop sizes', async ({page}, testInfo) => {
    for (const width of [800, 1440]) {
        for (const view of ['main','allmods','installmanager','options']) {
            await openView(page,view,{width});
            const ready = view === 'main' ? '#par:not(:disabled)' : view === 'allmods' ? '.library-mod-row' : view === 'options' ? '#FLAG-HASHCHECKS' : '.installation-copy';
            await expect(page.locator(ready).first()).toBeVisible();
            expect(await page.locator('.viewport').evaluate(node=>node.scrollWidth <= node.clientWidth + 1)).toBe(true);
            await expect(page.locator('.sidebar-button[data-page="main"] img')).toBeVisible();
            await page.screenshot({path:testInfo.outputPath(`${view}-${width}.png`)});
        }
    }
});

test('new search labels and counts change language in place', async ({page}) => {
    await openView(page,'main');
    await expect(page.locator('#par')).toBeEnabled();
    await page.evaluate(() => window.Localization.setLanguage('it'));
    await expect(page.locator('label[for="mod-search"]')).toHaveText('Cerca mod');
    await expect(page.locator('#mod-search-count')).toHaveText('3 di 3 mod');
});

test('shop suggestions debounce, encode the query, use the current game and support keyboard selection', async ({page}) => {
    await openView(page,'main');
    await page.locator('#mod-search').evaluate(input=>{ input.id='searchInput'; });
    await page.evaluate(() => {
        const menu = document.createElement('div');
        menu.className = 'results';
        document.querySelector('.mod-search-toolbar').append(menu);
        window.__suggestions = [];
        window.deltamodBackend.invoke = async () => ({gamebanana:{id:1234}});
        window.fetch = async (url, options) => {
            window.__suggestions.push({url,aborted:options.signal.aborted});
            return new Response(JSON.stringify(['Forest & friends','Forest redux']));
        };
    });
    const source = read('views/gamebanana-browse/index.js');
    const behavior = source.slice(source.indexOf('// Suggestions are input-driven'),source.lastIndexOf('})();'));
    await page.addScriptTag({content:`(() => {
        const searchel=document.querySelector('#searchInput'),autocomplete=document.querySelector('.results'),clearSearchButton=document.querySelector('#clear-mod-search');
        const SHOP_PROVIDER='gamebanana',isCurrentShopPage=()=>true,syncSearchClearButton=()=>{};
        const search=query=>{window.__selectedSuggestion=query;};
        ${behavior}
    })();`});
    await page.locator('#searchInput').fill('Forest &');
    await expect(page.locator('[role="option"]')).toHaveCount(2);
    const requests = await page.evaluate(()=>window.__suggestions);
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toContain('_idGameRow=1234');
    expect(requests[0].url).toContain('_sSearchString=Forest%20%26');
    await page.locator('#searchInput').press('ArrowDown');
    await page.locator('#searchInput').press('Enter');
    expect(await page.evaluate(()=>window.__selectedSuggestion)).toBe('Forest & friends');
    await expect(page.locator('#searchInput')).toHaveAttribute('aria-expanded','false');
});

test('navigation drops queued artwork and ignores late native results', async ({page}) => {
    await openView(page,'allmods',{count:0});
    await page.evaluate(() => {
        window.__resolvers = [];
        window.deltamodBackend.invoke = () => new Promise(resolve=>window.__resolvers.push(resolve));
        const loader = window.FrontendRefinements.artworkLoader();
        for(let i=0;i<12;i++) {
            const img = document.createElement('img');
            img.id=`lazy-${i}`; img.width=30; img.height=30; img.src=window.__placeholder;
            document.querySelector('.viewport').prepend(img); loader(img,`lazy-${i}`);
        }
    });
    await expect.poll(() => page.evaluate(()=>window.__resolvers.length)).toBe(4);
    await page.evaluate(() => {
        window._onClosePage.forEach(dispose=>dispose());
        window.__resolvers.forEach(resolve=>resolve({path:'should-not-load.png'}));
    });
    expect(await page.evaluate(()=>window.__resolvers.length)).toBe(4);
    expect(await page.locator('#lazy-0').getAttribute('src')).toMatch(/^data:/);
});
