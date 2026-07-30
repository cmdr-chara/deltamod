// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { test, expect, _electron: electron } = require('@playwright/test');
const fs = require('fs');
const os = require('os');
const path = require('path');

test('launches securely and keeps Options categories inside their column', async () => {
    test.setTimeout(90000);
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
    const gamePlatform = process.platform === 'darwin' ? 'darwin' : 'win32';
    const gameFiles = gamePlatform === 'darwin'
        ? [
            'DELTARUNE.app/Contents/MacOS/Mac_Runner',
            'DELTARUNE.app/Contents/Resources/game.ios'
        ]
        : ['DELTARUNE.exe', 'data.win'];
    for (const relativePath of gameFiles) {
        const target = path.join(gamePath, relativePath);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, '');
    }
    fs.writeFileSync(path.join(installationPath, '_cname'), 'Test installation');
    fs.writeFileSync(path.join(installationPath, 'store.json'), JSON.stringify({
        gamePid: 'toby.deltarune',
        gamePlatform,
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

        const resumedMenuAudioPositions = await window.evaluate(() => {
            const realAudio = audio;
            const silentAudio = {
                src: '',
                currentTime: 0,
                duration: 120,
                readyState: 1,
                pause() {},
                addEventListener() {},
                removeAttribute() {
                    this.src = '';
                },
                load() {}
            };

            audio = silentAudio;
            currentAudioSource = '';
            menuAudioPositions.clear();

            switchMenuAudioSource('themeprot://mus/waterfall.ogg');
            silentAudio.currentTime = 30;
            switchMenuAudioSource('./views/options/ost.mp3');
            silentAudio.currentTime = 12;
            switchMenuAudioSource('themeprot://mus/waterfall.ogg');
            const waterfall = silentAudio.currentTime;
            switchMenuAudioSource('./views/options/ost.mp3');
            const options = silentAudio.currentTime;
            releaseAudioBuffer();
            switchMenuAudioSource('./views/options/ost.mp3');
            const optionsAfterRelease = silentAudio.currentTime;

            audio = realAudio;
            currentAudioSource = '';
            menuAudioPositions.clear();
            return { waterfall, options, optionsAfterRelease };
        });
        expect(resumedMenuAudioPositions).toEqual({
            waterfall: 30,
            options: 12,
            optionsAfterRelease: 12
        });

        await window.evaluate(() => window.Localization.setLanguage('en'));
        const languageToggle = window.locator('#language-wheel-toggle');
        await expect(languageToggle).toBeVisible();
        const toggleLayout = await languageToggle.boundingBox();
        expect(toggleLayout.x).toBeLessThan(80);
        expect(toggleLayout.y + toggleLayout.height).toBeGreaterThan(
            (await window.evaluate(() => window.innerHeight)) - 80
        );

        await languageToggle.click();
        await expect(languageToggle).toHaveAttribute('aria-expanded', 'true');
        const languageWheel = window.locator('#language-wheel');
        await expect(languageWheel).toBeVisible();
        await expect(window.locator('.language-wheel-option')).toHaveCount(8);
        await expect(window.locator('.language-wheel-label')).toHaveCount(0);
        const viewport = await window.evaluate(() => ({
            width: window.innerWidth,
            height: window.innerHeight
        }));
        await expect.poll(() => languageWheel.evaluate(element => {
            const bounds = element.getBoundingClientRect();
            return {
                x: bounds.x + bounds.width / 2,
                y: bounds.y + bounds.height / 2
            };
        })).toEqual({
            x: viewport.width / 2,
            y: viewport.height / 2
        });
        await expect(window.locator('#language-wheel-current-flag')).toHaveAttribute(
            'src',
            /language-flags\/en\.svg$/
        );
        const wheelBounds = await languageWheel.boundingBox();
        const languageOrder = ['en', 'it', 'pl', 'es', 'fr', 'de', 'pt-br', 'ja'];
        const sectorPoint = index => {
            const angle = ((index * 45) - 90) * (Math.PI / 180);
            return {
                x: wheelBounds.x + 180 + (Math.cos(angle) * 112),
                y: wheelBounds.y + 180 + (Math.sin(angle) * 112)
            };
        };
        for (const [index, language] of languageOrder.entries()) {
            const point = sectorPoint(index);
            await expect.poll(() => window.evaluate(({ x, y }) => (
                document.elementFromPoint(x, y)?.dataset.language || null
            ), point)).toBe(language);
        }
        const japaneseSector = sectorPoint(languageOrder.indexOf('ja'));
        await window.mouse.move(japaneseSector.x, japaneseSector.y);
        await expect.poll(() => languageWheel.evaluate(element => (
            Number.parseFloat(element.style.getPropertyValue('--preview-angle'))
        ))).toBe(-45);
        const englishSector = sectorPoint(languageOrder.indexOf('en'));
        await window.mouse.move(englishSector.x, englishSector.y);
        await expect.poll(() => languageWheel.evaluate(element => (
            Number.parseFloat(element.style.getPropertyValue('--preview-angle'))
        ))).toBe(0);
        const italianSector = sectorPoint(languageOrder.indexOf('it'));
        await window.mouse.move(italianSector.x, italianSector.y);
        await expect(window.locator('#language-wheel-current-flag')).toHaveAttribute(
            'src',
            /language-flags\/it\.svg$/
        );
        const sectorGapAngle = -67.5 * (Math.PI / 180);
        await window.mouse.move(
            wheelBounds.x + 180 + (Math.cos(sectorGapAngle) * 112),
            wheelBounds.y + 180 + (Math.sin(sectorGapAngle) * 112)
        );
        await expect(window.locator('#language-wheel-current-flag')).toHaveAttribute(
            'src',
            /language-flags\/it\.svg$/
        );
        await window.evaluate(() => {
            const flag = document.getElementById('language-wheel-current-flag');
            window.__languagePreviewSources = [];
            window.__languagePreviewObserver = new MutationObserver(() => {
                window.__languagePreviewSources.push(flag.getAttribute('src'));
            });
            window.__languagePreviewObserver.observe(flag, {
                attributes: true,
                attributeFilter: ['src']
            });
        });
        const spanishSector = sectorPoint(languageOrder.indexOf('es'));
        await window.mouse.click(spanishSector.x, spanishSector.y);
        await expect.poll(() => window.evaluate(() => document.documentElement.lang)).toBe('es');
        await expect(window.locator('#language-wheel-toggle-flag')).toHaveAttribute('src', /langs\/es\/flag\.svg$/);
        await window.waitForTimeout(300);
        const previewSources = await window.evaluate(() => {
            window.__languagePreviewObserver.disconnect();
            return window.__languagePreviewSources;
        });
        expect(previewSources.length).toBeGreaterThan(0);
        expect(previewSources.every(source => /language-flags\/es\.svg$/.test(source))).toBe(true);

        await languageToggle.click();
        await window.mouse.click(englishSector.x, englishSector.y);
        await expect.poll(() => window.evaluate(() => document.documentElement.lang)).toBe('en');
        await expect(window.locator('#language-wheel-toggle-flag')).toHaveAttribute(
            'src',
            /langs\/en\/flag\.svg$/
        );

        await window.evaluate(() => page('options'));
        await window.locator('#b_lang').waitFor({ state: 'visible' });
        await window.evaluate(() => window.currentPageStack.cat('lang'));
        await expect(window.locator('.language-option-row')).toHaveCount(8);
        await expect(window.locator('.language-option-row').filter({ hasText: 'Español' })).toBeVisible();
        await expect(window.locator('.language-option-row').filter({ hasText: '日本語' })).toBeVisible();
        const italianLanguage = window.locator('.language-option-row').filter({ hasText: 'Italiano' });
        await italianLanguage.getByRole('button', { name: 'Select' }).click();
        await expect.poll(() => window.evaluate(() => document.documentElement.lang)).toBe('it');
        await expect(window.locator('.page-heading h1')).toHaveText('Opzioni');
        await expect(window.locator('#b_lang')).toHaveText('Lingua');
        const englishLanguage = window.locator('.language-option-row').filter({ hasText: 'English' });
        await englishLanguage.getByRole('button', { name: 'Seleziona' }).click();
        await expect.poll(() => window.evaluate(() => document.documentElement.lang)).toBe('en');

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

            await window.evaluate(() => page('themesel'));
            await expect(window.locator('#open-theme-import')).toBeVisible();
            const expectedThemeCount = await window.evaluate(async () =>
                (await window.electronAPI.invoke('getTheme', [])) === 'chara' ? 14 : 13
            );
            await expect(window.locator('.theme-card')).toHaveCount(expectedThemeCount);
            await expect(window.locator('.theme-card.is-current')).toHaveCount(1);
            await window.locator('#theme-filter').fill('Church');
            await expect(window.locator('.theme-card:visible')).toHaveCount(1);
            await expect(window.locator('.theme-card:visible')).toContainText('Church');
            await window.locator('#theme-filter').fill('');
            await expect(window.locator('.theme-card:visible')).toHaveCount(expectedThemeCount);
            await window.locator('#open-theme-import').click();
            await expect(window.locator('#theme-import-form')).toBeVisible();
            await expect(window.locator('#theme-import-name')).toBeFocused();
            await expect(window.locator('#theme-import-include-music')).not.toBeChecked();
            await expect(window.locator('.theme-import-copy')).toContainText(
                'Choose the background first'
            );
            await window.locator('#cancel-theme-import').click();
            await expect(window.locator('#theme-import-form')).toBeHidden();

            if (viewport.width === 1366) {
                const themeFilter = window.locator('#theme-filter');
                await themeFilter.fill('');
                await themeFilter.pressSequentially('chara');
                const charaEncounter = window.locator('#chara-easter-egg');
                await expect(charaEncounter).toBeVisible();
                await expect(themeFilter).toHaveValue('');
                await expect(themeFilter).toHaveAttribute('placeholder', 'THE TRUE NAME');
                await expect(charaEncounter).toHaveAttribute('data-phase', 'dialogue');
                await expect(window.locator('.chara-portrait')).toHaveAttribute(
                    'src',
                    /chara-easter-egg\/chara-normal\.png$/
                );
                if (process.env.DELTAMOD_SCREENSHOT_DIR) {
                    fs.mkdirSync(process.env.DELTAMOD_SCREENSHOT_DIR, { recursive: true });
                    await window.screenshot({
                        path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '09-chara-dialogue.png')
                    });
                }

                const charaChoices = window.locator('.chara-choices');
                for (let keyPress = 0; keyPress < 12 && !(await charaChoices.isVisible()); keyPress += 1) {
                    await charaEncounter.press('Enter');
                }
                await expect(charaChoices).toBeVisible();
                await expect(window.getByRole('button', { name: 'PROCEED' })).toBeFocused();
                await window.getByRole('button', { name: 'PROCEED' }).click();
                await expect(charaEncounter).toHaveAttribute('data-phase', 'proceed');
                await expect(charaEncounter).toHaveCount(0, { timeout: 3000 });
                await expect.poll(() =>
                    window.evaluate(() => window.electronAPI.invoke('getTheme', []))
                ).toBe('chara');
                await window.waitForFunction(() => window.pageN === 'main');
                const charaThemeVideo = window.locator('#theme-background-video');
                await expect(charaThemeVideo).toBeVisible();
                await expect(charaThemeVideo).toHaveAttribute(
                    'src',
                    'themeprot://video/chara-theme.mp4'
                );
                await expect.poll(() => charaThemeVideo.evaluate(video => video.readyState))
                    .toBeGreaterThan(0);
                expect(await charaThemeVideo.evaluate(video => video.muted)).toBe(true);

                await charaThemeVideo.evaluate(video => {
                    video.currentTime = Math.min(1, video.duration || 1);
                });
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.currentTime)
                ).toBeGreaterThan(0.5);
                await window.evaluate(async () => {
                    const video = document.getElementById('theme-background-video');
                    await page('options');
                });
                expect(await charaThemeVideo.evaluate(video => video.currentTime))
                    .toBeGreaterThan(0.5);
                await window.evaluate(() => window.currentPageStack.cat('ui'));
                const musicToggle = window.locator('#FLAG-AUDIO');
                await expect(musicToggle).not.toBeChecked();
                await charaThemeVideo.evaluate(video => {
                    video.currentTime = Math.min(3, video.duration || 3);
                });
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.currentTime)
                ).toBeGreaterThan(2.5);
                await musicToggle.check();
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.muted)
                ).toBe(false);
                await musicToggle.uncheck();
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.muted)
                ).toBe(true);
                const positionBeforeReenable = await charaThemeVideo.evaluate(video => video.currentTime);
                await musicToggle.check();
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.muted)
                ).toBe(false);
                expect(await charaThemeVideo.evaluate(video => video.currentTime))
                    .toBeGreaterThan(positionBeforeReenable - 0.5);
                await musicToggle.uncheck();
                await charaThemeVideo.evaluate(video => {
                    video.currentTime = Math.min(2, video.duration || 2);
                });
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.currentTime)
                ).toBeGreaterThan(1.5);
                const positionBeforeBlur = await charaThemeVideo.evaluate(video => video.currentTime);
                await window.evaluate(() => window.dispatchEvent(new Event('blur')));
                await window.waitForTimeout(1400);
                await expect(charaThemeVideo).not.toHaveAttribute('src');
                await expect(charaThemeVideo).toBeHidden();
                await window.evaluate(() => window.dispatchEvent(new Event('focus')));
                await expect(charaThemeVideo).toHaveAttribute(
                    'src',
                    'themeprot://video/chara-theme.mp4'
                );
                await expect(charaThemeVideo).toBeVisible();
                await expect.poll(() =>
                    charaThemeVideo.evaluate(video => video.currentTime)
                ).toBeGreaterThan(0.5);
                const positionAfterFocus = await charaThemeVideo.evaluate(video => video.currentTime);
                expect(positionAfterFocus).toBeGreaterThan(positionBeforeBlur - 0.5);

                await window.evaluate(() => page('themesel'));
                const charaThemeCard = window.locator('.theme-card.is-current');
                await expect(charaThemeCard).toContainText('Chara');
                await charaThemeCard.locator('.theme-credits summary').click();
                await expect(charaThemeCard.locator('.theme-credits')).toContainText('Clara Kraft');
            }

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
            await window.evaluate(pageName => {
                if (pageName === 'allmods') window._pageArguments = null;
                return page(pageName);
            }, route);
            await window.waitForFunction(pageName => window.pageN === pageName, route);
            await expect(window.locator(selector)).toBeVisible();
            if (route === 'options') {
                await window.waitForFunction(() =>
                    typeof window.currentPageStack?.cat === 'function'
                );
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
        await window.evaluate(async () => {
            await window.electronAPI.invoke('setTheme', ['church']);
            await themeRefresh(false);
        });
        await window.waitForFunction(() =>
            theme?.id === 'church' &&
            document.querySelector('.dmodicon')?.src.startsWith('data:image/png')
        );
        const themedSpriteColors = await window.evaluate(() => {
            const readColors = image => {
                const canvas = document.createElement('canvas');
                canvas.width = image.naturalWidth;
                canvas.height = image.naturalHeight;
                const context = canvas.getContext('2d');
                context.drawImage(image, 0, 0);
                const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
                const colors = new Set();
                for (let index = 0; index < pixels.length; index += 4) {
                    if (pixels[index + 3] > 0) {
                        colors.add(`${pixels[index]},${pixels[index + 1]},${pixels[index + 2]}`);
                    }
                }
                return [...colors];
            };
            return {
                gear: readColors(document.querySelector('.dmodicon')),
                options: readColors(document.querySelector('[data-page="options"] img'))
            };
        });
        expect(themedSpriteColors.gear).toContain('0,60,255');
        expect(themedSpriteColors.gear).toContain('116,122,160');
        expect(themedSpriteColors.options).toContain('0,60,255');
        expect(themedSpriteColors.options).toContain('255,201,14');
        await window.evaluate(async () => {
            await window.electronAPI.invoke('setTheme', ['home']);
            await themeRefresh(false);
        });

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
        const providerSelectLayout = await window.locator('#modSourceSelect').evaluate(select => ({
            clientWidth: select.clientWidth,
            scrollWidth: select.scrollWidth
        }));
        expect(providerSelectLayout.scrollWidth).toBeLessThanOrEqual(providerSelectLayout.clientWidth);
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
            localStorage.setItem('modShopProvider', 'nexus');
            window._pageArguments = { provider: 'nexus' };
            page('gamebanana-browse');
        });
        await expect(window.locator('#modSourceSelect')).toHaveValue('nexus');
        await expect(window.getByRole('heading', { name: 'Connect Nexus Mods' })).toBeVisible();
        await expect(window.locator('#modsBody')).not.toContainText('Error invoking remote method');
        await expect(window.getByRole('button', { name: 'Open Nexus Mods settings' })).toBeVisible();
        await expect(window.locator('#searchInput')).toBeDisabled();
        await expect(window.locator('#nexusSort')).toBeDisabled();
        await expect(window.locator('.shop-page thead')).toBeHidden();

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

        await window.evaluate(() => page('themesel'));
        await window.setViewportSize({ width: 873, height: 558 });
        const finalThemeFilter = window.locator('#theme-filter');
        await finalThemeFilter.waitFor({ state: 'visible' });
        await finalThemeFilter.pressSequentially('chara');
        const finalEncounter = window.locator('#chara-easter-egg');
        await expect(finalEncounter).toBeVisible();
        const finalChoices = window.locator('.chara-choices');
        for (let keyPress = 0; keyPress < 12 && !(await finalChoices.isVisible()); keyPress += 1) {
            await finalEncounter.press('Enter');
        }
        await expect(finalChoices).toBeVisible();
        const choicePresentation = await finalChoices.evaluate(element => {
            const bounds = element.getBoundingClientRect();
            const buttonStyle = getComputedStyle(element.querySelector('.chara-choice'));
            return {
                fitsViewport: bounds.top >= 0 && bounds.bottom <= window.innerHeight,
                backgroundColor: buttonStyle.backgroundColor,
                borderWidth: buttonStyle.borderWidth
            };
        });
        expect(choicePresentation.fitsViewport).toBe(true);
        expect(choicePresentation.backgroundColor).toBe('rgba(0, 0, 0, 0)');
        expect(choicePresentation.borderWidth).toBe('0px');
        const windowPositionBeforeStrike = await application.evaluate(({ BrowserWindow }) =>
            BrowserWindow.getAllWindows()[0].getPosition()
        );
        const applicationClosed = application.waitForEvent('close');
        await window.getByRole('button', { name: 'GO BACK' }).click();
        await expect(finalEncounter).toHaveAttribute('data-phase', 'refusal');
        await expect(finalEncounter).toHaveAttribute('data-phase', 'scare', { timeout: 4000 });
        await expect(finalEncounter).toHaveAttribute('data-phase', 'numbers', { timeout: 7000 });
        const windowPositionDuringNumbers = await application.evaluate(({ BrowserWindow }) =>
            BrowserWindow.getAllWindows()[0].getPosition()
        );
        expect(windowPositionDuringNumbers).not.toEqual(windowPositionBeforeStrike);
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '10-chara-9999.png')
            });
        }
        await applicationClosed;
        application = null;
    } finally {
        if (application) await application.close();
        fs.rmSync(userData, { recursive: true, force: true });
    }
});
