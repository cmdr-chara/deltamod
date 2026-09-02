// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { test, expect, _electron: electron } = require('@playwright/test');
const fs = require('fs');
const os = require('os');
const path = require('path');

test('launches securely and keeps Options categories inside their column', async () => {
    test.setTimeout(120000);
    const builtInThemes = fs.readdirSync(path.join(__dirname, '..', '..', 'web', 'themes', 'data'))
        .filter(fileName => fileName.endsWith('.theme.json'))
        .map(fileName => JSON.parse(fs.readFileSync(
            path.join(__dirname, '..', '..', 'web', 'themes', 'data', fileName),
            'utf8'
        )));
    const defaultVisibleThemeCount = builtInThemes.filter(theme => !theme.hiddenByDefault).length;
    const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-community-e2e-'));
    // A partially initialized or migrated profile may contain the parent folder
    // before any custom theme assets have been created.
    fs.mkdirSync(path.join(userData, 'customThemes'));
    const uniqueDataPath = path.join(userData, 'deltamod_system-unique');
    fs.mkdirSync(uniqueDataPath);
    fs.writeFileSync(path.join(uniqueDataPath, 'flagDB.config'), [
        'AUDIO = 0',
        'SFX = 0'
    ].join('\n'));
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
                ? { executablePath: packagedExecutable, args: ['--mute-audio'] }
                : { args: ['--mute-audio', '.'] }),
            env: {
                ...process.env,
                DELTAMOD_TEST: '1',
                DELTAMOD_TEST_ALLOW_AUDIO: '1',
                DELTAMOD_TEST_USER_DATA: userData,
                DELTAMOD_NEXUS_OAUTH_CLIENT_ID: 'deltamod-community-test'
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
        const bootEmblem = window.locator('.animated-pixel-heart');
        await expect(bootEmblem).toBeVisible({ timeout: 15000 });
        const bootEmblemSurface = await bootEmblem.evaluate(element => {
            const canvas = element.querySelector('canvas');
            const rootStyle = getComputedStyle(element);
            const canvasStyle = getComputedStyle(canvas);
            return {
                disabled: element.disabled,
                background: rootStyle.backgroundColor,
                borderWidth: rootStyle.borderTopWidth,
                boxShadow: rootStyle.boxShadow,
                canvasBackground: canvasStyle.backgroundColor
            };
        });
        expect(bootEmblemSurface).toEqual({
            disabled: true,
            background: 'rgba(0, 0, 0, 0)',
            borderWidth: '0px',
            boxShadow: 'none',
            canvasBackground: 'rgba(0, 0, 0, 0)'
        });
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
        const seasonalLayer = window.locator('#deltamod-seasonal-layer');
        await window.evaluate(() => window.SeasonalEvents.setMode('christmas'));
        await expect(seasonalLayer).toBeVisible();
        await expect(seasonalLayer).toHaveAttribute('data-event', 'christmas');
        await expect(window.locator('.dmodicon')).toHaveClass(/dmodicon-seasonal-active/);
        await expect(window.locator('.seasonal-dmodicon-glyph')).toHaveAttribute('data-event', 'christmas');
        const seasonalThemeColors = await seasonalLayer.evaluate(layer => {
            const probe = document.createElement('span');
            probe.style.color = 'var(--theme-color)';
            document.body.appendChild(probe);
            const theme = getComputedStyle(probe).color;
            probe.style.color = 'var(--theme-soul-color)';
            const themeSoul = getComputedStyle(probe).color;
            probe.remove();
            return {
                accent: getComputedStyle(layer.querySelector('.seasonal-corner-primary')).color,
                soul: getComputedStyle(layer.querySelector('.seasonal-corner-secondary')).color,
                shell: getComputedStyle(document.querySelector('.seasonal-dmodicon-glyph')).color,
                theme,
                themeSoul
            };
        });
        expect(seasonalThemeColors.accent).toBe(seasonalThemeColors.theme);
        expect(seasonalThemeColors.soul).toBe(seasonalThemeColors.themeSoul);
        expect(seasonalThemeColors.shell).toBe(seasonalThemeColors.themeSoul);

        const shellAlignment = await window.evaluate(() => {
            const icon = document.querySelector('.dmodicon').getBoundingClientRect();
            const glyph = document.querySelector('.seasonal-dmodicon-glyph').getBoundingClientRect();
            return {
                x: Math.abs((icon.left + icon.width / 2) - (glyph.left + glyph.width / 2)),
                y: Math.abs((icon.top + icon.height / 2) - (glyph.top + glyph.height / 2))
            };
        });
        expect(shellAlignment.x).toBeLessThanOrEqual(2);
        expect(shellAlignment.y).toBeLessThanOrEqual(2);

        const liveSeasonalColors = await window.evaluate(() => {
            const root = document.documentElement;
            root.style.setProperty('--theme-color', 'rgb(7, 111, 213)');
            root.style.setProperty('--theme-soul-color', 'rgb(214, 63, 147)');
            const result = {
                accent: getComputedStyle(document.querySelector('.seasonal-corner-primary')).color,
                soul: getComputedStyle(document.querySelector('.seasonal-corner-secondary')).color,
                shell: getComputedStyle(document.querySelector('.seasonal-dmodicon-glyph')).color
            };
            root.style.removeProperty('--theme-color');
            root.style.removeProperty('--theme-soul-color');
            return result;
        });
        expect(liveSeasonalColors).toEqual({
            accent: 'rgb(7, 111, 213)',
            soul: 'rgb(214, 63, 147)',
            shell: 'rgb(214, 63, 147)'
        });
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            fs.mkdirSync(process.env.DELTAMOD_SCREENSHOT_DIR, { recursive: true });
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '00-seasonal-christmas-boot.png')
            });
        }
        await expect(window.locator('#deltamod-boot-root')).toBeHidden({ timeout: 12000 });
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '00-seasonal-christmas-ui.png')
            });
        }

        await window.evaluate(async () => page('options'));
        await window.waitForFunction(() => window.pageN === 'options');
        await window.evaluate(async () => window.currentPageStack.cat('ui'));
        const seasonalMode = window.locator('#SELECT-SEASONAL-MODE');
        await expect(seasonalMode).toHaveValue('christmas');
        await seasonalMode.selectOption('womens-health');
        await expect(window.locator('.seasonal-dmodicon-glyph')).toHaveAttribute('data-event', 'womens-health');
        const womensHealthShadow = await window.locator('.seasonal-corner-primary .seasonal-pixel-mark')
            .evaluate(mark => getComputedStyle(mark).boxShadow);
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '01-seasonal-womens-health.png')
            });
        }
        await seasonalMode.selectOption('mens-health');
        await expect(window.locator('.seasonal-dmodicon-glyph')).toHaveAttribute('data-event', 'mens-health');
        const mensHealthShadow = await window.locator('.seasonal-corner-primary .seasonal-pixel-mark')
            .evaluate(mark => getComputedStyle(mark).boxShadow);
        expect(mensHealthShadow).not.toBe(womensHealthShadow);
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '02-seasonal-mens-health.png')
            });
        }
        await seasonalMode.selectOption('off');
        await expect(seasonalLayer).toBeHidden();
        await expect(window.locator('.dmodicon')).not.toHaveClass(/dmodicon-seasonal-active/);
        await seasonalMode.selectOption('auto');
        await expect(seasonalMode).toHaveValue('auto');
        await window.evaluate(async () => page('main'));
        await expect.poll(() => window.evaluate(async () => ({
            music: await window.electronAPI.invoke('getUniqueFlag', ['AUDIO']),
            sfx: await window.electronAPI.invoke('getUniqueFlag', ['SFX'])
        }))).toEqual({ music: false, sfx: false });

        const dismissSoundCount = await window.evaluate(async () => {
            const originalPlay = HTMLMediaElement.prototype.play;
            let dismissSounds = 0;
            HTMLMediaElement.prototype.play = function() {
                if (this.src.endsWith('/audio/booow.mp3')) dismissSounds += 1;
                return Promise.resolve();
            };
            await window.electronAPI.invoke('setUniqueFlag', ['SFX', true]);
            try {
                const alertResult = htmlAlert('Sound test', 'Dismiss this dialog.', [
                    { text: 'Close', resolveWith: 'closed' }
                ]);
                document.querySelector('.alertButtons button').click();
                await alertResult;
                return dismissSounds;
            } finally {
                await window.electronAPI.invoke('setUniqueFlag', ['SFX', false]);
                HTMLMediaElement.prototype.play = originalPlay;
            }
        });
        expect(dismissSoundCount).toBe(1);

        const resumedMenuAudioPositions = await window.evaluate(() => {
            const realAudio = audio;
            const silentAudio = {
                src: '',
                currentTime: 0,
                duration: 120,
                readyState: 1,
                pause() {},
                addEventListener(type, listener) {
                    if (type === 'loadedmetadata') this.loadedMetadataListener = listener;
                },
                removeAttribute() {
                    this.src = '';
                },
                load() {
                    const listener = this.loadedMetadataListener;
                    this.loadedMetadataListener = null;
                    listener?.();
                }
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
        await window.waitForFunction(() => document.getElementById('deltamod-boot-root')?.hidden === true);
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
        await expect.poll(() => languageWheel.evaluate((element, viewportSize) => {
            const bounds = element.getBoundingClientRect();
            return Math.hypot(
                (bounds.x + bounds.width / 2) - (viewportSize.width / 2),
                (bounds.y + bounds.height / 2) - (viewportSize.height / 2)
            );
        }, viewport)).toBeLessThanOrEqual(1);
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
        await expect(window.locator('#language-wheel-toggle-flag')).toHaveAttribute(
            'src',
            /language-flags\/es\.svg$/
        );
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
            /language-flags\/en\.svg$/
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
            expect(transformedAnimations.some(animation =>
                String(animation.targetClass).includes('ingranaggio')
            )).toBe(true);
            expect(transformedAnimations.some(animation =>
                String(animation.targetClass).split(/\s+/).includes('bg')
            )).toBe(false);
            expect(transformedAnimations.every(animation =>
                ['ingranaggio', 'dmodicon-ring', 'theme-environment-sprite'].some(className =>
                    String(animation.targetClass).includes(className)
                )
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
            const expectedThemeCount = defaultVisibleThemeCount + await window.evaluate(async () =>
                (await window.electronAPI.invoke('getTheme', [])) === 'chara' ? 1 : 0
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
                'Set its identity here'
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
                await expect(charaEncounter).toBeFocused();
                await charaEncounter.press('Tab');
                await expect(charaEncounter).toBeFocused();
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

                await charaEncounter.press('Escape');
                await expect(charaEncounter).toHaveCount(0);
                await expect(themeFilter).toBeFocused();
                await expect(themeFilter).toHaveAttribute(
                    'placeholder',
                    'Name, description, or music'
                );
                await themeFilter.pressSequentially('chara');
                const replayedCharaEncounter = window.locator('#chara-easter-egg');
                await expect(replayedCharaEncounter).toBeVisible();

                const charaChoices = window.locator('.chara-choices');
                for (let keyPress = 0; keyPress < 12 && !(await charaChoices.isVisible()); keyPress += 1) {
                    await replayedCharaEncounter.press('Enter');
                }
                await expect(charaChoices).toBeVisible();
                await expect(replayedCharaEncounter).toHaveAttribute('data-phase', 'choice');
                const proceedChoice = window.getByRole('button', { name: 'PROCEED' });
                const goBackChoice = window.getByRole('button', { name: 'GO BACK' });
                await expect(proceedChoice).toBeFocused();
                await proceedChoice.press('Tab');
                await expect(goBackChoice).toBeFocused();
                await goBackChoice.press('Shift+Tab');
                await expect(proceedChoice).toBeFocused();
                await proceedChoice.click();
                await expect(replayedCharaEncounter).toHaveAttribute('data-phase', 'proceed');
                await expect(replayedCharaEncounter).toHaveCount(0, { timeout: 3000 });
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
                await expect(charaThemeVideo).toHaveAttribute(
                    'src',
                    'themeprot://video/chara-theme.mp4'
                );
                await expect(charaThemeVideo).toBeVisible();
                expect(await charaThemeVideo.evaluate(video => video.paused)).toBe(true);
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

        await application.evaluate(({ ipcMain }) => {
            const makeGameBananaMod = ({ id, name, description, authorId, authorName, imageCount }) => ({
                _idRow: id,
                _sModelName: 'Mod',
                _sName: name,
                _sDescription: description,
                _sProfileUrl: `https://gamebanana.com/mods/${id}`,
                _bHasFiles: true,
                _bHasContentRatings: false,
                _tsDateAdded: 1751328000 + (id === 41 ? 1 : 0),
                _tsDateModified: 1751328000 + (id === 41 ? 1 : 0),
                _aSubmitter: {
                    _idRow: authorId,
                    _sName: authorName,
                    _sProfileUrl: `https://gamebanana.com/members/${authorId}`,
                    _sAvatarUrl: './img/mod-placeholder.png'
                },
                _aPreviewMedia: {
                    _aImages: Array.from({ length: imageCount }, () => ({
                        _sBaseUrl: './img',
                        _sFile: 'mod-placeholder.png',
                        _sFile100: 'mod-placeholder.png',
                        _sFile220: 'mod-placeholder.png',
                        _sFile530: 'mod-placeholder.png'
                    }))
                }
            });
            const regularMod = makeGameBananaMod({
                id: 41,
                name: 'Regular test mod',
                description: 'A regular GameBanana card.',
                authorId: 8,
                authorName: 'Regular author',
                imageCount: 1
            });
            const galleryMod = makeGameBananaMod({
                id: 42,
                name: 'Gallery test mod',
                description: 'A GameBanana card using the shared shop layout.',
                authorId: 7,
                authorName: 'Test author',
                imageCount: 2
            });

            ipcMain.removeHandler('modSources:browse');
            ipcMain.handle('modSources:browse', (_event, args) => {
                const request = args?.[0] || {};
                if (request.provider !== 'gamebanana') {
                    return {
                        ok: false,
                        error: {
                            code: 'TEST_UNEXPECTED_PROVIDER',
                            message: `Unexpected provider in E2E fixture: ${request.provider || 'unknown'}`
                        }
                    };
                }

                const url = String(request.url || '');
                let payload;
                if (url.includes('/Subfeed')) {
                    payload = {
                        _aMetadata: { _bIsComplete: true },
                        _aRecords: url.includes('_nPage=2') ? [galleryMod] : [regularMod]
                    };
                } else if (url.includes('/TopSubs')) {
                    payload = [{ ...galleryMod, _sPeriod: 'alltime' }];
                } else {
                    return {
                        ok: false,
                        error: {
                            code: 'TEST_UNEXPECTED_GAMEBANANA_URL',
                            message: `Unexpected GameBanana URL in E2E fixture: ${url}`
                        }
                    };
                }

                return {
                    ok: true,
                    result: {
                        provider: 'gamebanana',
                        payload,
                        cached: false,
                        stale: false
                    }
                };
            });
        });

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
            if (route === 'main') {
                const patchMenuLayout = await window.evaluate(() => {
                    const toolbar = document.querySelector('.patch-toolbar').getBoundingClientRect();
                    const actions = document.querySelector('.patch-actions').getBoundingClientRect();
                    const table = document.querySelector('#modtable').getBoundingClientRect();
                    return {
                        actionsInsideToolbar: actions.top >= toolbar.top
                            && actions.right <= toolbar.right + 1
                            && actions.bottom <= toolbar.bottom + 1,
                        tableGap: table.top - toolbar.bottom
                    };
                });
                expect(patchMenuLayout.actionsInsideToolbar).toBe(true);
                expect(patchMenuLayout.tableGap).toBeGreaterThanOrEqual(0);
                expect(patchMenuLayout.tableGap).toBeLessThanOrEqual(24);
            }
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
                await expect(window.locator('#modsBody tr').first()).toContainText('Gallery test mod');
                await expect(window.locator('#modsBody')).toContainText('Regular test mod');
                await window.evaluate(() => window.currentPageStack.plusPage(1));
                await expect(window.locator('#modsBody')).toContainText('Gallery test mod');
                await expect(window.locator('#modsBody tr').filter({ hasText: 'Gallery test mod' })).toHaveCount(1);
                const firstMod = window.locator('#modsBody tr').first();
                await expect(firstMod).toContainText('Gallery test mod');
                await expect(firstMod).toContainText('GameBanana');
                await expect(firstMod).toContainText('All-time featured');
                await expect(firstMod).toContainText('A GameBanana card using the shared shop layout.');
                await expect(window.locator('.mod-gallery-count').first()).toHaveText('+1');
                await expect(window.locator('.modThumbGrid')).toHaveCount(0);
                await window.getByRole('button', { name: 'Preview Gallery test mod (2 images)' }).first().click();
                await expect(window.locator('#modImageLightboxCounter')).toHaveText('1 of 2');
                await window.keyboard.press('Escape');
                await expect(window.locator('#modImageLightbox')).not.toBeVisible();

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

        const navigationSpam = await window.evaluate(async () => {
            const routes = [
                'main',
                'allmods',
                'options',
                'installmanager',
                'gamebanana-browse',
                'credits'
            ];
            const results = await Promise.all(routes.map(route => page(route)));
            const viewport = document.querySelector('.viewport');
            const settledMarkup = viewport.innerHTML;
            let mutations = 0;
            const observer = new MutationObserver(records => {
                mutations += records.length;
            });
            observer.observe(viewport, { childList: true, subtree: true });
            const activeButton = document.querySelector('.sidebar-button[data-page="credits"]');
            for (let index = 0; index < 12; index += 1) {
                activeButton.click();
            }
            observer.disconnect();

            return {
                results,
                page: window.pageN,
                activePage: document.querySelector('.sidebar-button.active')?.dataset.page,
                script: document.querySelector('script[data-page-script]')?.dataset.pageScript,
                markupUnchanged: viewport.innerHTML === settledMarkup,
                mutations
            };
        });
        expect(navigationSpam.page).toBe('credits');
        expect(navigationSpam.activePage).toBe('credits');
        expect(navigationSpam.script).toBe('credits');
        expect(navigationSpam.results.filter(Boolean)).toHaveLength(2);
        expect(navigationSpam.markupUnchanged).toBe(true);
        expect(navigationSpam.mutations).toBe(0);
        await expect(window.locator('#credits')).toBeVisible();

        await window.evaluate(() => page('gamebanana-browse'));
        await window.waitForFunction(() => window.pageN === 'gamebanana-browse');
        await window.getByRole('button', { name: 'Return home' }).click();
        await window.waitForFunction(() => window.pageN === 'main');
        await expect(window.locator('#modtable')).toBeVisible();

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

        await window.evaluate(() => {
            window.fetch = window.__deltamodOriginalRendererFetch;
            delete window.__deltamodOriginalRendererFetch;
        });

        await window.evaluate(() => window.electronAPI.invoke('setTheme', ['base']));
        await expect.poll(() => window.evaluate(() => window.electronAPI.invoke('getTheme', []))).toBe('base');
        await window.evaluate(async () => {
            await window.electronAPI.invoke('setTheme', ['church']);
            await themeRefresh(false);
        });
        await window.waitForFunction(() =>
            theme?.id === 'church' &&
            document.querySelector('.dmodicon-ring')?.src.startsWith('data:image/png')
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
                gear: readColors(document.querySelector('.dmodicon-ring')),
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
        await window.evaluate(async () => {
            localStorage.setItem('modShopProvider', 'moddb');
            window._pageArguments = { provider: 'moddb' };
            await page('gamebanana-browse');
        });
        await expect(window.locator('#modSourceSelect')).toHaveValue('moddb');
        await expect(window.locator('#modSourceSelect option:checked')).toHaveText('ModDB (10 recent)');
        await expect(window.locator('#modsBody')).toContainText('Gendered Kris');
        await expect(window.locator('#contentFilterStatus')).toContainText('exposed by ModDB');
        await expect(window.locator('#sourceAttribution')).toContainText('10 most recent downloads');
        await expect(window.getByRole('link', { name: 'Browse the full ModDB catalogue' })).toBeVisible();
        const providerSelectLayout = await window.locator('#modSourceSelect').evaluate(select => ({
            clientWidth: select.clientWidth,
            scrollWidth: select.scrollWidth
        }));
        expect(providerSelectLayout.scrollWidth).toBeLessThanOrEqual(providerSelectLayout.clientWidth);
        const shopActionIcons = window.locator(
            '.shop-toolbar-icon-button > .shop-action-icon, '
            + '.external-source-actions button > .shop-action-icon'
        );
        const shopActionIconGeometry = await shopActionIcons.evaluateAll(icons => icons.map(icon => {
            const iconRect = icon.getBoundingClientRect();
            const buttonRect = icon.parentElement.getBoundingClientRect();
            const style = getComputedStyle(icon);
            return {
                width: iconRect.width,
                height: iconRect.height,
                x: iconRect.left + (iconRect.width / 2) - (buttonRect.left + (buttonRect.width / 2)),
                y: iconRect.top + (iconRect.height / 2) - (buttonRect.top + (buttonRect.height / 2)),
                fill: style.fill,
                stroke: style.stroke
            };
        }));
        expect(shopActionIconGeometry).toHaveLength(3);
        for (const icon of shopActionIconGeometry) {
            expect(icon.width).toBe(20);
            expect(icon.height).toBe(20);
            expect(Math.abs(icon.x)).toBeLessThanOrEqual(0.05);
            expect(Math.abs(icon.y)).toBeLessThanOrEqual(0.05);
            expect(icon.fill).toBe('none');
            expect(icon.stroke).not.toBe('none');
        }
        const externalActionGeometry = await window.locator('.external-source-actions button').first().evaluate(button => ({
            width: button.getBoundingClientRect().width,
            height: button.getBoundingClientRect().height,
            radius: getComputedStyle(button).borderRadius
        }));
        expect(externalActionGeometry.width).toBe(36);
        expect(externalActionGeometry.height).toBe(36);
        expect(externalActionGeometry.radius).toBe('50%');        await window.evaluate(() => window.currentPageStack.updateModDownloadStatus({
            phase: 'download',
            completed: 524288,
            total: 1048576,
            currentItem: 'sample-mod.zip'
        }));
        await expect(window.locator('#modDownloadStatus')).toBeVisible();
        await expect(window.locator('#modDownloadStatusTitle')).toHaveText('Downloading mod…');
        await expect(window.locator('#modDownloadStatusItem')).toHaveText('sample-mod.zip');
        await expect(window.locator('#modDownloadStatusBytes')).toHaveText('512 KB / 1 MB');
        await expect(window.locator('#modDownloadStatusPercent')).toHaveText('50%');
        await expect(window.locator('#modDownloadProgressTrack')).toHaveAttribute('aria-valuenow', '50');
        await window.evaluate(() => window.currentPageStack.updateModDownloadStatus({
            phase: 'import',
            completed: 1048576,
            total: 1048576,
            currentItem: 'sample-mod.zip'
        }));
        await expect(window.locator('#modDownloadStatusTitle')).toHaveText('Importing mod…');
        await window.evaluate(() => window.currentPageStack.updateModDownloadStatus({
            phase: 'complete',
            completed: 1048576,
            total: 1048576,
            currentItem: 'sample-mod.zip'
        }));
        await expect(window.locator('#modDownloadStatusTitle')).toHaveText('Mod imported successfully');
        await expect(window.locator('#modDownloadStatusPercent')).toHaveText('100%');
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
        await application.evaluate(() => {
            globalThis.__deltamodOriginalFetch = globalThis.fetch;
            globalThis.fetch = async (input, options) => {
                if (String(input).includes('api.nexusmods.com/v2/graphql')) {
                    return new Response(JSON.stringify({
                        data: {
                            mods: {
                                totalCount: 2,
                                nodes: [
                                    {
                                        modId: 23,
                                        name: 'Deltarune - Kris Gender Mod CHAPTER 5',
                                        summary: 'Choose masculine or feminine text for Kris.',
                                        author: 'Ryzex',
                                        updatedAt: '2026-06-27T20:20:42Z',
                                        pictureUrl: null,
                                        adultContent: false,
                                        downloads: 23,
                                        endorsements: 2
                                    },
                                    {
                                        modId: 53,
                                        name: 'Nexus mod 53',
                                        summary: '',
                                        author: 'Seanbot10',
                                        updatedAt: '2026-07-29T14:23:00Z',
                                        pictureUrl: null,
                                        adultContent: false,
                                        downloads: 530,
                                        endorsements: 53
                                    }
                                ]
                            }
                        }
                    }), {
                        status: 200,
                        headers: { 'content-type': 'application/json' }
                    });
                }
                return globalThis.__deltamodOriginalFetch(input, options);
            };
        });

        await window.evaluate(async () => {
            localStorage.setItem('modShopProvider', 'nexus');
            window._pageArguments = { provider: 'nexus' };
            await page('gamebanana-browse');
        });
        await expect(window.locator('#modSourceSelect')).toHaveValue('nexus');
        await expect(window.locator('#modsBody')).toContainText('Deltarune - Kris Gender Mod CHAPTER 5');
        await expect(window.locator('#modsBody tr').first()).toContainText('Deltarune - Kris Gender Mod CHAPTER 5');
        await expect(window.locator('#modsBody tr').first()).toContainText('Featured');
        await expect(window.locator('#modsBody tr').first()).toContainText('23 downloads · 2 endorsements');
        await expect(window.locator('#nexusSort')).toHaveValue('trending');
        await expect(window.locator('#searchInput')).toBeEnabled();
        await expect(window.locator('#nexusSort')).toBeEnabled();
        await expect(window.locator('.shop-page thead')).toBeVisible();
        const nexusSearch = window.locator('#searchInput');
        const clearSearch = window.getByRole('button', { name: 'Clear search' });
        await nexusSearch.fill('kris');
        await expect(clearSearch).toBeVisible();
        await clearSearch.click();
        await expect(window.locator('#searchInput')).toHaveValue('');
        await expect(window.getByRole('button', { name: 'Clear search' })).toBeHidden();
        await expect(window.locator('#modsBody')).toContainText('Nexus mod 53');
        await window.getByRole('button', {
            name: 'Download: Deltarune - Kris Gender Mod CHAPTER 5'
        }).click();
        await expect(window.locator('.alertMsg h1')).toHaveText('Nexus Mods authorization required');
        await expect(window.locator('.alertMsg p')).toContainText('continue on the official mod page');
        await expect(window.getByRole('button', { name: 'Open Nexus Mods' })).toBeVisible();
        await window.getByRole('button', { name: 'Cancel' }).click();
        await application.evaluate(() => {
            globalThis.fetch = globalThis.__deltamodOriginalFetch;
            delete globalThis.__deltamodOriginalFetch;
        });

        await window.evaluate(async () => {
            window._pageArguments = { cat: 'nexus' };
            await page('options');
        });
        await window.locator('#b_nexus').waitFor({ state: 'visible' });
        await window.waitForFunction(() =>
            typeof window.currentPageStack?.cat === 'function'
        );
        await window.evaluate(() => window.currentPageStack.cat('nexus'));
        await expect(window.locator('.nexus-key-row input')).toHaveCount(0);
        await expect(window.locator('#options')).toContainText('Not connected');
        await expect(window.getByRole('button', { name: 'Sign in' })).toBeVisible();
        await expect(window.locator('#options')).toContainText('returns through Community’s fixed local callback');
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '08-nexus-settings.png')
            });
        }
        await window.evaluate(() => localStorage.setItem('modShopProvider', 'gamebanana'));

        const productFixture = JSON.parse(fs.readFileSync(path.join(
            __dirname,
            '..',
            '..',
            'src-tauri',
            'crates',
            'product-contracts',
            'tests',
            'fixtures',
            'contracts-v1.json'
        ), 'utf8'));
        await window.evaluate(async fixture => {
            const loadStyles = () => new Promise((resolve, reject) => {
                const existing = document.querySelector('link[data-e2e-product-ui]');
                if (existing) {
                    resolve();
                    return;
                }
                const link = document.createElement('link');
                link.rel = 'stylesheet';
                link.href = './modules/product-ui.css';
                link.setAttribute('data-e2e-product-ui', '');
                link.addEventListener('load', resolve, { once: true });
                link.addEventListener('error', reject, { once: true });
                document.head.appendChild(link);
            });
            const loadScript = () => window.DeltamodProductUI
                ? Promise.resolve()
                : new Promise((resolve, reject) => {
                    const script = document.createElement('script');
                    script.src = './modules/product-ui.js';
                    script.addEventListener('load', resolve, { once: true });
                    script.addEventListener('error', reject, { once: true });
                    document.body.appendChild(script);
                });
            await Promise.all([loadStyles(), loadScript()]);

            const preview = document.createElement('section');
            preview.id = 'installed-mods-v2-browser-preview';
            preview.className = 'content-page installed-mods-page product-ui';
            Object.assign(preview.style, {
                position: 'fixed',
                inset: '24px',
                zIndex: '5000',
                boxSizing: 'border-box',
                maxWidth: 'none',
                overflow: 'auto',
                padding: '24px',
                background: 'rgb(8, 8, 8)'
            });
            const root = document.createElement('main');
            preview.appendChild(root);
            document.body.appendChild(preview);
            const model = window.DeltamodProductUI.mapContractsV1Fixture(fixture, {
                availableVersionByInstanceId: { 'fixture-instance': '1.1.0' }
            });
            window.DeltamodProductUI.renderInstalledModsV2(root, model, {
                locale: document.documentElement.lang || 'en'
            });
        }, productFixture);
        const preview = window.locator('#installed-mods-v2-browser-preview');
        const healthSummary = preview.locator('.product-health-summary');
        await expect(healthSummary).toBeVisible();
        await expect(healthSummary).toHaveAttribute('data-health-state', 'healthy');
        await expect(healthSummary).toContainText('Managed files1');
        await expect(healthSummary).toContainText('External changes0');
        await expect(healthSummary).toContainText('Interrupted operations0');
        const healthBeforeOperation = await window.evaluate(() => {
            const health = document.querySelector('.product-health-summary');
            const operation = document.querySelector('.product-operation-progress');
            return Boolean(
                health &&
                operation &&
                (health.compareDocumentPosition(operation) & Node.DOCUMENT_POSITION_FOLLOWING)
            );
        });
        expect(healthBeforeOperation).toBe(true);
        const lifecycleActions = preview.locator('[data-lifecycle-action]');
        await expect(lifecycleActions).toHaveCount(4);
        for (let actionIndex = 0; actionIndex < 4; actionIndex += 1) {
            await expect(lifecycleActions.nth(actionIndex)).toBeDisabled();
        }
        const conflictTrigger = preview.getByRole('button', { name: 'Review conflicts' });
        await conflictTrigger.click();
        const conflictDialog = preview.locator('.product-conflict-dialog');
        await expect(conflictDialog).toBeVisible();
        await expect(conflictDialog).toContainText('mods/fixture.dat');
        await conflictDialog.press('Escape');
        await expect(conflictDialog).toBeHidden();
        await expect(conflictTrigger).toBeFocused();
        await window.setViewportSize({ width: 640, height: 720 });
        const installedModsOverflow = await window.evaluate(() => {
            const page = document.querySelector('.installed-mods-page');
            return page.scrollWidth - page.clientWidth;
        });
        expect(installedModsOverflow).toBeLessThanOrEqual(1);
        if (process.env.DELTAMOD_SCREENSHOT_DIR) {
            await window.screenshot({
                path: path.join(process.env.DELTAMOD_SCREENSHOT_DIR, '09-installed-mods-v2.png')
            });
        }
        await preview.evaluate(element => element.remove());

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
        await window.getByRole('button', { name: 'GO BACK' }).click();
        await expect(finalEncounter).toHaveAttribute('data-phase', 'refusal');
        await expect(finalEncounter).toHaveAttribute('data-phase', 'scare', { timeout: 4000 });
        await expect(finalEncounter).toHaveAttribute('data-phase', 'numbers', { timeout: 7000 });
        const windowPositionDuringNumbers = await application.evaluate(({ BrowserWindow }) =>
            BrowserWindow.getAllWindows()[0].getPosition()
        );
        const usesRendererShakeFallback = await window.evaluate(() => (
            document.querySelector('#chara-easter-egg')?.classList.contains('is-window-shake-fallback')
        ));
        if (usesRendererShakeFallback) {
            expect(windowPositionDuringNumbers).toEqual(windowPositionBeforeStrike);
        } else {
            expect(windowPositionDuringNumbers).not.toEqual(windowPositionBeforeStrike);
        }
        await finalEncounter.press('Escape');
        await expect(finalEncounter).toHaveCount(0);
        await expect(finalThemeFilter).toBeFocused();
        await expect.poll(() => application.evaluate(({ BrowserWindow }) =>
            BrowserWindow.getAllWindows()[0].getPosition()
        )).toEqual(windowPositionBeforeStrike);

        await finalThemeFilter.pressSequentially('chara');
        const closingEncounter = window.locator('#chara-easter-egg');
        await expect(closingEncounter).toBeVisible();
        const closingChoices = window.locator('.chara-choices');
        for (let keyPress = 0; keyPress < 12 && !(await closingChoices.isVisible()); keyPress += 1) {
            await closingEncounter.press('Enter');
        }
        await expect(closingChoices).toBeVisible();
        const applicationClosed = application.waitForEvent('close');
        await window.getByRole('button', { name: 'GO BACK' }).click();
        await expect(closingEncounter).toHaveAttribute('data-phase', 'numbers', { timeout: 7000 });
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
