// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { test, expect, _electron: electron } = require('@playwright/test');
const fs = require('fs');
const os = require('os');
const path = require('path');

test('launches securely and keeps Options categories inside their column', async () => {
    const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-community-e2e-'));
    // A partially initialized or migrated profile may contain the parent folder
    // before any custom theme assets have been created.
    fs.mkdirSync(path.join(userData, 'customThemes'));
    const gamePath = path.join(userData, 'game');
    const installationPath = path.join(userData, 'deltamod_system-0');
    const officialInstallationPath = path.join(userData, 'appData', 'deltamod', 'deltamod_system-0');
    fs.mkdirSync(gamePath);
    fs.mkdirSync(installationPath);
    fs.mkdirSync(officialInstallationPath, { recursive: true });
    fs.writeFileSync(path.join(gamePath, 'DELTARUNE.exe'), '');
    fs.writeFileSync(path.join(gamePath, 'data.win'), '');
    fs.writeFileSync(path.join(installationPath, '_cname'), 'Test installation');
    fs.writeFileSync(path.join(installationPath, 'store.json'), JSON.stringify({
        gamePid: 'toby.deltarune',
        deltaruneEdition: 'rem',
        version: 'DELTAMOD_DATA_2.0.2',
        loadedDeltarune: true,
        gamePath,
        enabledMods: [],
        isSteam: false,
        steamAppId: ''
    }));
    fs.writeFileSync(path.join(officialInstallationPath, 'store.json'), JSON.stringify({
        version: 'DELTAMOD_DATA_1.7.0'
    }));
    let application;
    const pageErrors = [];
    try {
        const packagedExecutable = process.env.DELTAMOD_E2E_EXECUTABLE;
        application = await electron.launch({
            ...(packagedExecutable
                ? { executablePath: packagedExecutable, args: [] }
                : { args: ['.'] }),
            env: {
                ...process.env,
                DELTAMOD_TEST: '1',
                DELTAMOD_TEST_USER_DATA: userData,
                DELTAMOD_NEXUS_SSO_APP_ID: 'deltamod-community-test'
            }
        });
        const window = await application.firstWindow();
        window.on('console', message => console.log(`[renderer:${message.type()}] ${message.text()}`));
        window.on('pageerror', error => {
            pageErrors.push(error.stack || error.message);
            console.error(`[renderer:error] ${error.stack || error.message}`);
        });
        window.on('response', response => {
            if (response.status() >= 400) {
                console.error(`[renderer:http] ${response.status()} ${response.url()}`);
            }
        });
        await window.waitForLoadState('domcontentloaded');
        await expect(window).toHaveTitle('Deltamod Community');
        const migrationLaterButton = window.getByRole('button', { name: 'Later' });
        await window.waitForFunction(() =>
            window.pageN === 'main' ||
            [...document.querySelectorAll('button')].some(button => button.textContent?.trim() === 'Later')
        );
        if (await migrationLaterButton.isVisible()) {
            await migrationLaterButton.click();
            await window.waitForTimeout(400);
        }
        await window.waitForFunction(() => window.pageN === 'main');
        await expect.poll(() => window.evaluate(() => window.electronAPI.invoke('getTheme', []))).toBe('home');
        await expect.poll(() => window.evaluate(async () => ({
            music: await window.electronAPI.invoke('getUniqueFlag', ['AUDIO']),
            sfx: await window.electronAPI.invoke('getUniqueFlag', ['SFX'])
        }))).toEqual({ music: false, sfx: false });

        for (const viewport of [
            { width: 900, height: 600 },
            { width: 1366, height: 768 },
            { width: 1920, height: 1080 }
        ]) {
            await window.setViewportSize(viewport);
            await window.evaluate(() => page('options'));
            await window.locator('#b_ui').waitFor({ state: 'visible' });
            const transformedAnimations = await window.evaluate(() =>
                document.getAnimations()
                    .map(animation => ({
                        targetClass: animation.effect?.target?.className || '',
                        transforms: (animation.effect?.getKeyframes?.() || [])
                            .map(frame => frame.transform)
                            .filter(transform => transform && transform !== 'none')
                    }))
                    .filter(animation => animation.transforms.length > 0)
            );
            expect(transformedAnimations.length).toBeGreaterThanOrEqual(1);
            expect(transformedAnimations.every(animation =>
                String(animation.targetClass).includes('ingranaggio')
            )).toBe(true);
            await window.evaluate(() => window.currentPageStack.cat('ui'));
            const layout = await window.evaluate(() => {
                const selected = document.querySelector('#b_ui').getBoundingClientRect();
                const content = document.querySelector('.opt').getBoundingClientRect();
                const style = getComputedStyle(document.querySelector('#b_ui'));
                const rowAlignment = [...document.querySelectorAll('#options tr')]
                    .map(row => [...row.cells].map(cell => cell.getBoundingClientRect()))
                    .filter(cells => cells.length === 2)
                    .map(cells => Math.abs(cells[0].bottom - cells[1].bottom));
                return {
                    overlapsContent:
                        selected.left < content.right &&
                        selected.right > content.left &&
                        selected.top < content.bottom &&
                        selected.bottom > content.top,
                    transform: style.transform,
                    rowAlignment,
                    overflowWidth: document.documentElement.scrollWidth,
                    viewportWidth: document.documentElement.clientWidth
                };
            });
            expect(layout.overlapsContent).toBe(false);
            expect(layout.transform).toBe('none');
            expect(layout.rowAlignment.every(delta => delta <= 1)).toBe(true);
            expect(layout.overflowWidth).toBeLessThanOrEqual(layout.viewportWidth + 1);

            await window.evaluate(() => window.currentPageStack.cat('data'));
            await expect(window.locator('.profile-destination-path')).toBeVisible();
            const destinationLayout = await window.locator('.profile-destination-path').evaluate(element => {
                const path = element.getBoundingClientRect();
                const cell = element.parentElement.getBoundingClientRect();
                return {
                    insideCell: path.left >= cell.left && path.right <= cell.right + 1,
                    overflowsHorizontally: element.scrollWidth > element.clientWidth + 1,
                    hasBreakOpportunities: element.querySelectorAll('wbr').length,
                    separatorCount: (element.textContent.match(/[\\/]/g) || []).length
                };
            });
            expect(destinationLayout.insideCell).toBe(true);
            expect(destinationLayout.overflowsHorizontally).toBe(false);
            expect(destinationLayout.separatorCount).toBeGreaterThan(0);
            expect(destinationLayout.hasBreakOpportunities).toBe(destinationLayout.separatorCount);

            await window.evaluate(() => page('credits'));
            await expect(window.locator('#maintainer-title')).toContainText('Chara');
            await expect(window.locator('#maintainerProfileButton')).toBeVisible();
            const creditsLayout = await window.evaluate(() => {
                const avatar = document.querySelector('#maintainerAvatar').getBoundingClientRect();
                return {
                    avatarWidth: avatar.width,
                    avatarHeight: avatar.height,
                    avatarRadius: getComputedStyle(document.querySelector('#maintainerAvatar')).borderRadius,
                    overflowWidth: document.documentElement.scrollWidth,
                    viewportWidth: document.documentElement.clientWidth,
                    clippedNames: [...document.querySelectorAll('.credit-person span')]
                        .filter(element => element.scrollWidth > element.clientWidth + 1)
                        .map(element => element.textContent)
                };
            });
            expect(creditsLayout.avatarWidth).toBeGreaterThanOrEqual(76);
            expect(creditsLayout.avatarHeight).toBe(creditsLayout.avatarWidth);
            expect(creditsLayout.avatarRadius).toBe('50%');
            expect(creditsLayout.overflowWidth).toBeLessThanOrEqual(creditsLayout.viewportWidth + 1);
            expect(creditsLayout.clippedNames).toEqual([]);
        }

        await window.setViewportSize({ width: 1920, height: 1080 });
        const shell = await window.evaluate(() => ({
            sidebarWidth: document.querySelector('.sidebar').getBoundingClientRect().width,
            viewportBorderWidth: getComputedStyle(document.querySelector('.viewport')).borderLeftWidth,
            visibleSidebarLabels: [...document.querySelectorAll('.sidebar-button')]
                .filter(button => getComputedStyle(button).display !== 'none')
                .some(button => button.textContent.trim().length > 0)
        }));
        expect(shell.sidebarWidth).toBe(70);
        expect(shell.viewportBorderWidth).toBe('0px');
        expect(shell.visibleSidebarLabels).toBe(false);

        const getZoomFactor = () => application.evaluate(({ BrowserWindow }) =>
            BrowserWindow.getAllWindows()[0].webContents.getZoomFactor()
        );
        const sendZoomShortcut = keyCode => application.evaluate(({ BrowserWindow }, shortcutKey) => {
            const contents = BrowserWindow.getAllWindows()[0].webContents;
            contents.focus();
            contents.sendInputEvent({ type: 'keyDown', keyCode: shortcutKey, modifiers: ['control'] });
            contents.sendInputEvent({ type: 'keyUp', keyCode: shortcutKey, modifiers: ['control'] });
        }, keyCode);
        await sendZoomShortcut('+');
        await expect.poll(getZoomFactor).toBeCloseTo(1.1, 5);
        await sendZoomShortcut('-');
        await expect.poll(getZoomFactor).toBeCloseTo(1, 5);
        await sendZoomShortcut('+');
        await sendZoomShortcut('0');
        await expect.poll(getZoomFactor).toBeCloseTo(1, 5);

        for (const [route, selector] of [
            ['main', '#modtable'],
            ['allmods', '#modtable'],
            ['options', '#options'],
            ['installmanager', '#installations-list'],
            ['gamebanana-browse', '#modsBody'],
            ['credits', '#credits']
        ]) {
            await window.evaluate(pageName => page(pageName), route);
            await window.waitForFunction(pageName => window.pageN === pageName, route);
            await expect(window.locator(selector)).toBeVisible();
            if (route === 'options') {
                await window.evaluate(() => window.currentPageStack.cat('ui'));
            }
            if (route === 'allmods') {
                const filterWidth = await window.locator('#gamesShow').evaluate(element =>
                    element.getBoundingClientRect().width
                );
                expect(filterWidth).toBeLessThanOrEqual(320);
            }
            if (route === 'gamebanana-browse') {
                await window.evaluate(() => window.currentPageStack.openImageLightbox(
                    'Test mod preview',
                    [
                        { urlA: './img/mod-placeholder.png', urlB: './img/mod-placeholder.png' },
                        { urlA: './img/mod-placeholder.png', urlB: './img/mod-placeholder.png' }
                    ],
                    0
                ));
                await expect(window.locator('#modImageLightbox')).toBeVisible();
                await expect(window.locator('#modImageLightboxTitle')).toHaveText('Test mod preview');
                await expect(window.locator('#modImageLightboxCounter')).toHaveText('1 of 2');
                await window.locator('#modImageZoomIn').click();
                await expect(window.locator('#modImageZoomLevel')).toHaveText('125%');
                await window.keyboard.press('ArrowRight');
                await expect(window.locator('#modImageLightboxCounter')).toHaveText('2 of 2');
                await window.keyboard.press('Escape');
                await expect(window.locator('#modImageLightbox')).not.toBeVisible();
            }
            const overflow = await window.evaluate(() => ({
                width: document.documentElement.scrollWidth,
                viewport: document.documentElement.clientWidth
            }));
            expect(overflow.width).toBeLessThanOrEqual(overflow.viewport + 1);

            if (process.env.DELTAMOD_SCREENSHOT_DIR) {
                fs.mkdirSync(process.env.DELTAMOD_SCREENSHOT_DIR, { recursive: true });
                await window.waitForTimeout(220);
                const index = String([
                    'main',
                    'allmods',
                    'options',
                    'installmanager',
                    'gamebanana-browse',
                    'credits'
                ].indexOf(route) + 1).padStart(2, '0');
                await window.screenshot({
                    path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, `${index}-${route}.png`)
                });
            }
        }

        for (const viewport of [
            { width: 900, height: 600 },
            { width: 1366, height: 768 }
        ]) {
            await window.setViewportSize(viewport);
            for (const [route, selector] of [
                ['main', '#modtable'],
                ['allmods', '#modtable'],
                ['options', '#options'],
                ['installmanager', '#installations-list'],
                ['gamebanana-browse', '#modsBody'],
                ['credits', '#credits']
            ]) {
                await window.evaluate(pageName => page(pageName), route);
                await window.waitForFunction(pageName => window.pageN === pageName, route);
                await expect(window.locator(selector)).toBeVisible();
                const overflow = await window.evaluate(() => ({
                    width: document.documentElement.scrollWidth,
                    viewport: document.documentElement.clientWidth
                }));
                expect(overflow.width).toBeLessThanOrEqual(overflow.viewport + 1);
            }
        }

        await window.evaluate(() => window.electronAPI.invoke('setTheme', ['base']));
        await expect.poll(() => window.evaluate(() => window.electronAPI.invoke('getTheme', []))).toBe('base');
        await window.evaluate(() => window.electronAPI.invoke('setTheme', ['home']));

        const modDbFixture = `<?xml version="1.0"?>
            <rss version="2.0">
              <channel>
                <item>
                  <title>Gendered Kris</title>
                  <link>https://www.moddb.com/games/deltarune/downloads/gendered-kris</link>
                  <pubDate>Fri, 06 Jun 2025 06:26:11 +0000</pubDate>
                  <guid isPermaLink="false">downloads291264</guid>
                  <description><![CDATA[Test ModDB catalogue result.]]></description>
                </item>
              </channel>
            </rss>`;
        await application.evaluate(({ app }, fixture) => {
            globalThis.__deltamodOriginalFetch = globalThis.fetch;
            globalThis.fetch = async (input, options) => {
                if (String(input).includes('rss.moddb.com')) {
                    return new Response(fixture, {
                        status: 200,
                        headers: { 'content-type': 'application/rss+xml' }
                    });
                }
                return globalThis.__deltamodOriginalFetch(input, options);
            };
        }, modDbFixture);
        await window.setViewportSize({ width: 900, height: 600 });
        await window.evaluate(() => {
            localStorage.setItem('modShopProvider', 'moddb');
            window._pageArguments = { provider: 'moddb' };
            page('gamebanana-browse');
        });
        await expect(window.locator('#modSourceSelect')).toHaveValue('moddb');
        await expect(window.locator('#modSourceSelect option:checked')).toHaveText('ModDB (recent)');
        await expect(window.locator('#modsBody')).toContainText('Gendered Kris');
        await expect(window.locator('#contentFilterStatus')).toContainText('recent ModDB');
        await expect(window.locator('#sourceAttribution')).toContainText('not the complete ModDB catalogue');
        await expect(window.getByRole('link', { name: 'Browse the full ModDB catalogue' })).toBeVisible();
        const modDbOverflow = await window.evaluate(() => ({
            width: document.documentElement.scrollWidth,
            viewport: document.documentElement.clientWidth
        }));
        expect(modDbOverflow.width).toBeLessThanOrEqual(modDbOverflow.viewport + 1);
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            fs.mkdirSync(process.env.DELTAMOD_SCREENSHOT_DIR, { recursive: true });
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '07-moddb.png')
            });
        }
        await application.evaluate(() => {
            globalThis.fetch = globalThis.__deltamodOriginalFetch;
            delete globalThis.__deltamodOriginalFetch;
        });

        await window.evaluate(() => {
            page('options');
            window._pageArguments = { cat: 'nexus' };
        });
        await window.locator('#b_nexus').waitFor({ state: 'visible' });
        await window.evaluate(() => window.currentPageStack.cat('nexus'));
        await expect(window.locator('.nexus-key-row input')).toBeVisible();
        await expect(window.locator('#options')).toContainText('Not connected');
        await expect(window.getByRole('button', { name: 'Sign in' })).toBeVisible();
        await expect(window.locator('#options')).toContainText('no API key needs to be copied');
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '08-nexus-settings.png')
            });
        }
        await window.evaluate(() => localStorage.setItem('modShopProvider', 'gamebanana'));

        await window.evaluate(() => page('deleteall'));
        await expect(window.locator('#initbtn')).toBeVisible();
        expect(pageErrors).toEqual([]);
        await window.evaluate(() => page('deleteall'));
        await expect(window.locator('#initbtn')).toBeVisible();
    } finally {
        if (application) await application.close();
        fs.rmSync(userData, { recursive: true, force: true });
    }
});
