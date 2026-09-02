// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

(() => {
    const CHARA_ASSET_ROOT = './assets/chara-easter-egg';
    const CHARA_DIALOGUE = Object.freeze([
        'INTERESTING.',
        'YOU FOUND A NAME\nTHAT WAS NEVER LISTED.',
        'YOU GAVE THIS PROJECT\nANOTHER FUTURE.',
        'AND NOW...\nIT HAS NOTICED YOU.',
        'SHALL WE MAKE\nTHIS THEME OURS?'
    ]);
    const CHARA_UNLOCK_FLAG = 'CHARA_THEME_UNLOCKED';
    const UNDERTALE_THEME_IDS = new Set([
        'chara',
        'undertale',
        'undertale-core',
        'undertale-hotland',
        'undertale-new-home',
        'undertale-ruins',
        'undertale-snowdin',
        'undertale-true-lab',
        'undertale-void',
        'undertale-waterfall'
    ]);
    let charaBuffer = '';
    let charaDetected = false;
    let charaUnlocked = false;
    let charaEncounterActive = false;
    let pendingCharaToken = null;
    const charaSessionGate = window.DeltamodCharaEncounterSession.createSessionGate();
    const charaUnlockReady = window.deltamodBackend
        .invoke('getUniqueFlag', [CHARA_UNLOCK_FLAG])
        .then(value => {
            charaUnlocked = Boolean(value);
            return charaUnlocked;
        })
        .catch(() => false);
    const t = (key, fallback, ...args) => window.Localization?.t(key, fallback, ...args)
        ?? String(fallback).replace(/{(\d+)}/g, (match, index) => (
            args[index] === undefined ? match : String(args[index])
        ));
    const themeFilterPlaceholder = () => charaEncounterActive
        ? 'THE TRUE NAME'
        : t('theme_filter_placeholder', 'Name, description, or music');
    const refreshThemeFilterPlaceholder = () => {
        const filter = document.getElementById('theme-filter');
        if (filter) filter.placeholder = themeFilterPlaceholder();
    };

    function createCrossfadedAudioLoop(url, volume, crossfadeSeconds = 0.15) {
        let audioContext = null;
        let sourceNode = null;
        let gainNode = null;
        let fallbackAudio = null;
        let generation = 0;

        const stop = () => {
            generation += 1;
            if (sourceNode) {
                try {
                    sourceNode.stop();
                } catch {}
                sourceNode.disconnect();
                sourceNode = null;
            }
            gainNode?.disconnect();
            gainNode = null;
            if (fallbackAudio) {
                fallbackAudio.pause();
                fallbackAudio.currentTime = 0;
                fallbackAudio = null;
            }
            const contextToClose = audioContext;
            audioContext = null;
            contextToClose?.close().catch(() => {});
        };

        const play = async () => {
            stop();
            const playGeneration = generation;
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            if (!AudioContextClass) {
                fallbackAudio = new Audio(url);
                fallbackAudio.loop = true;
                fallbackAudio.volume = volume;
                await fallbackAudio.play();
                return;
            }

            const context = new AudioContextClass();
            audioContext = context;
            const response = await fetch(url);
            if (!response.ok) throw new Error(`Unable to load audio loop: ${response.status}`);
            const decoded = await context.decodeAudioData(await response.arrayBuffer());
            if (generation !== playGeneration || audioContext !== context) {
                await context.close();
                return;
            }

            const crossfadeFrames = Math.min(
                Math.max(1, Math.round(decoded.sampleRate * crossfadeSeconds)),
                Math.floor(decoded.length / 4)
            );
            const loopFrames = decoded.length - crossfadeFrames;
            const loopBuffer = context.createBuffer(
                decoded.numberOfChannels,
                loopFrames,
                decoded.sampleRate
            );

            for (let channel = 0; channel < decoded.numberOfChannels; channel += 1) {
                const input = decoded.getChannelData(channel);
                const output = loopBuffer.getChannelData(channel);
                output.set(input.subarray(0, loopFrames));
                for (let frame = 0; frame < crossfadeFrames; frame += 1) {
                    const mix = (frame + 1) / (crossfadeFrames + 1);
                    output[frame] = (
                        input[loopFrames + frame] * (1 - mix)
                        + input[frame] * mix
                    );
                }
            }

            sourceNode = context.createBufferSource();
            gainNode = context.createGain();
            sourceNode.buffer = loopBuffer;
            sourceNode.loop = true;
            gainNode.gain.value = volume;
            sourceNode.connect(gainNode);
            gainNode.connect(context.destination);
            await context.resume();
            sourceNode.start();
        };

        return { play, stop };
    }

    function staticIcon(name, size = '20px') {
        const glyph = document.createElement('span');
        glyph.className = 'material-symbols-outlined';
        glyph.style.fontSize = size;
        glyph.textContent = name;
        glyph.setAttribute('aria-hidden', 'true');
        return glyph;
    }

    function safeThemeBackground(theme) {
        const fileName = String(theme?.previewBackground || theme?.background || '');
        if (!/^[a-z0-9._-]+$/i.test(fileName)) return '';
        const url = theme?.builtIn
            ? window.deltamodBackend.assetUrl('app', `web/themes/img/${fileName}`)
            : window.deltamodBackend.assetUrl(
                'theme',
                theme?.runtimeLayout ? `${theme.id}/${fileName}` : `img/${fileName}`
            );
        return `url("${url}")`;
    }

    function safeCreditUrl(value) {
        try {
            const parsed = new URL(String(value || ''));
            return parsed.protocol === 'https:' ? parsed.href : '';
        } catch {
            return '';
        }
    }

    (async () => {
        refreshThemeFilterPlaceholder();
        const [availableThemes, currentTheme] = await Promise.all([
            window.deltamodBackend.invoke('getThemes', []),
            window.deltamodBackend.invoke('getTheme', []),
            charaUnlockReady
        ]);
        const themes = availableThemes.sort((a, b) => {
            if (a.builtIn && !b.builtIn) return -1;
            if (!a.builtIn && b.builtIn) return 1;
            if (a.timed && !b.timed) return -1;
            if (!a.timed && b.timed) return 1;
            const aExpired = a.timed && Date.now() > a.timedExpire;
            const bExpired = b.timed && Date.now() > b.timedExpire;
            if (!aExpired && bExpired) return -1;
            if (aExpired && !bExpired) return 1;
            return a.name.localeCompare(b.name);
        });
        let selectedTheme = currentTheme;
        const themeGrid = document.getElementById('themes');
        const filterInput = document.getElementById('theme-filter');
        const countLabel = document.getElementById('theme-count');
        const emptyState = document.getElementById('theme-empty');
        const categoryButtons = [...document.querySelectorAll('[data-theme-category]')];
        const themeCards = [];
        let selectedCategory = 'all';

        for (const theme of themes) {
            const isUnlockedChara = theme.id === 'chara' && charaUnlocked;
            if (theme.hiddenByDefault && theme.id !== selectedTheme && !isUnlockedChara) continue;

            const card = document.createElement('article');
            card.className = 'theme-card';
            card.dataset.themeId = theme.id;
            card.dataset.category = theme.builtIn
                ? (UNDERTALE_THEME_IDS.has(theme.id) ? 'undertale' : 'deltarune')
                : 'custom';
            card.dataset.search = [
                theme.name,
                theme.description,
                theme.musicTrack,
                ...(Array.isArray(theme.credits)
                    ? theme.credits.flatMap(credit => [credit.role, credit.name])
                    : []),
                theme.builtIn ? 'built-in' : 'custom'
            ].join(' ').toLocaleLowerCase();
            if (theme.id === selectedTheme) card.classList.add('is-current');

            const preview = document.createElement('div');
            preview.className = 'theme-card-preview';
            preview.style.backgroundImage = safeThemeBackground(theme);
            preview.setAttribute('role', 'img');
            preview.setAttribute(
                'aria-label',
                t('theme_background_preview', '{0} background preview', theme.name)
            );
            card.appendChild(preview);

            const content = document.createElement('div');
            content.className = 'theme-card-content';

            const heading = document.createElement('div');
            heading.className = 'theme-card-heading';
            const accent = document.createElement('span');
            accent.className = 'theme-accent';
            accent.style.backgroundColor = theme.color;
            accent.title = theme.soulColor
                ? t('theme_accent_colors', 'UI accent: {0}; SOUL color: {1}', theme.color, theme.soulColor)
                : t('theme_accent_only', 'Theme accent: {0}', theme.color);
            const name = document.createElement('h2');
            name.textContent = theme.name;
            heading.append(accent, name);

            const source = document.createElement('span');
            source.className = 'theme-source';
            source.textContent = theme.builtIn
                ? t('theme_built_in', 'Built-in')
                : t('theme_custom', 'Custom');
            heading.appendChild(source);
            content.appendChild(heading);

            const description = document.createElement('p');
            description.className = 'theme-description';
            description.textContent = theme.description;
            content.appendChild(description);

            const music = document.createElement('div');
            music.className = 'theme-music';
            music.appendChild(staticIcon('audio_file', '18px'));
            const musicName = document.createElement('span');
            musicName.textContent = theme.musicTrack;
            music.appendChild(musicName);
            content.appendChild(music);

            if (Array.isArray(theme.credits) && theme.credits.length > 0) {
                const credits = document.createElement('details');
                credits.className = 'theme-credits';
                const summary = document.createElement('summary');
                summary.textContent = t('theme_credits', 'Credits');
                const list = document.createElement('ul');

                for (const credit of theme.credits) {
                    const item = document.createElement('li');
                    const role = document.createElement('span');
                    role.textContent = `${credit.role}: `;
                    item.appendChild(role);

                    const creditUrl = safeCreditUrl(credit.url);
                    const name = creditUrl
                        ? document.createElement('a')
                        : document.createElement('span');
                    name.textContent = credit.name;
                    if (creditUrl) {
                        name.href = creditUrl;
                        name.target = '_blank';
                        name.rel = 'noreferrer';
                    }
                    item.appendChild(name);
                    list.appendChild(item);
                }

                credits.append(summary, list);
                content.appendChild(credits);
            }

            if (!theme.builtIn) {
                name.contentEditable = true;
                description.contentEditable = true;
                name.classList.add('is-editable');
                description.classList.add('is-editable');
                name.title = t('theme_edit_name', 'Click to edit the theme name');
                description.title = t('theme_edit_description', 'Click to edit the description');

                const saveDetails = async () => {
                    if (!name.textContent.trim()) name.textContent = theme.name;
                    if (!description.textContent.trim()) {
                        description.textContent = theme.description;
                    }
                    await window.deltamodBackend.invoke('renameCustomTheme', [
                        theme.id,
                        name.textContent.trim(),
                        description.textContent.trim()
                    ]);
                };
                name.addEventListener('blur', saveDetails);
                description.addEventListener('blur', saveDetails);
            }

            card.appendChild(content);

            const actions = document.createElement('div');
            actions.className = 'theme-card-actions';
            const selectButton = document.createElement('button');
            selectButton.type = 'button';
            selectButton.className = 'theme-select-button noScaleBTN';
            selectButton.textContent = theme.id === selectedTheme
                ? t('theme_in_use', 'In use')
                : t('theme_use', 'Use theme');
            selectButton.disabled = theme.id === selectedTheme;
            selectButton.setAttribute('aria-pressed', String(theme.id === selectedTheme));
            selectButton.addEventListener('click', async () => {
                await window.deltamodBackend.invoke('setTheme', [theme.id]);
                selectedTheme = theme.id;
                for (const candidate of themeCards) {
                    const active = candidate.dataset.themeId === theme.id;
                    candidate.classList.toggle('is-current', active);
                    const candidateButton = candidate.querySelector('.theme-select-button');
                    candidateButton.disabled = active;
                    candidateButton.textContent = active
                        ? t('theme_in_use', 'In use')
                        : t('theme_use', 'Use theme');
                    candidateButton.setAttribute('aria-pressed', String(active));
                }
                await themeRefresh(true);
            });
            actions.appendChild(selectButton);

            if (!theme.builtIn) {
                const deleteButton = document.createElement('button');
                deleteButton.type = 'button';
                deleteButton.className = 'theme-delete-button secondary-action noScaleBTN';
                deleteButton.appendChild(staticIcon('delete', '19px'));
                const deleteLabel = document.createElement('span');
                deleteLabel.textContent = t('theme_delete', 'Delete');
                deleteButton.appendChild(deleteLabel);
                deleteButton.addEventListener('click', async () => {
                    if (!window.confirm(t(
                        'theme_delete_confirm',
                        'Delete "{0}"? This cannot be undone.',
                        theme.name
                    ))) return;
                    if (theme.id === selectedTheme) {
                        await window.deltamodBackend.invoke('setTheme', ['base']);
                        await themeRefresh(true);
                    }
                    await window.deltamodBackend.invoke('deleteCustomTheme', [theme.id]);
                    await page('themesel');
                });
                actions.appendChild(deleteButton);
            }

            card.appendChild(actions);
            themeGrid.appendChild(card);
            themeCards.push(card);
        }

        const updateFilter = () => {
            const query = filterInput.value.trim().toLocaleLowerCase();
            let visible = 0;
            for (const card of themeCards) {
                const matchesQuery = !query || card.dataset.search.includes(query);
                const matchesCategory = selectedCategory === 'all'
                    || card.dataset.category === selectedCategory;
                const matches = matchesQuery && matchesCategory;
                card.hidden = !matches;
                if (matches) visible += 1;
            }
            countLabel.textContent = t(
                'theme_count',
                '{0} of {1} themes',
                visible,
                themeCards.length
            );
            emptyState.hidden = visible !== 0;
        };
        filterInput.addEventListener('input', updateFilter);
        for (const button of categoryButtons) {
            button.addEventListener('click', () => {
                selectedCategory = button.dataset.themeCategory;
                for (const candidate of categoryButtons) {
                    const active = candidate === button;
                    candidate.classList.toggle('is-active', active);
                    candidate.setAttribute('aria-pressed', String(active));
                }
                updateFilter();
            });
        }
        const updateDynamicTranslations = () => {
            filterInput.placeholder = themeFilterPlaceholder();
            importName.placeholder = t('theme_name_placeholder', 'My theme');
            importDescription.placeholder = t(
                'theme_description_placeholder',
                'What is this theme based on?'
            );
            const categoryLabels = {
                all: t('theme_category_all', 'All'),
                deltarune: 'DELTARUNE',
                undertale: 'UNDERTALE',
                custom: t('theme_custom', 'Custom')
            };
            for (const button of categoryButtons) {
                button.textContent = categoryLabels[button.dataset.themeCategory];
            }
            for (const card of themeCards) {
                const theme = themes.find(candidate => candidate.id === card.dataset.themeId);
                card.querySelector('.theme-card-preview').setAttribute(
                    'aria-label',
                    t('theme_background_preview', '{0} background preview', theme.name)
                );
                const accent = card.querySelector('.theme-accent');
                accent.title = theme.soulColor
                    ? t('theme_accent_colors', 'UI accent: {0}; SOUL color: {1}', theme.color, theme.soulColor)
                    : t('theme_accent_only', 'Theme accent: {0}', theme.color);
                card.querySelector('.theme-source').textContent = theme.builtIn
                    ? t('theme_built_in', 'Built-in')
                    : t('theme_custom', 'Custom');
                const summary = card.querySelector('.theme-credits summary');
                if (summary) summary.textContent = t('theme_credits', 'Credits');
                const editableName = card.querySelector('h2.is-editable');
                const editableDescription = card.querySelector('.theme-description.is-editable');
                if (editableName) editableName.title = t('theme_edit_name', 'Click to edit the theme name');
                if (editableDescription) {
                    editableDescription.title = t(
                        'theme_edit_description',
                        'Click to edit the description'
                    );
                }
                const selectButton = card.querySelector('.theme-select-button');
                selectButton.textContent = selectButton.disabled
                    ? t('theme_in_use', 'In use')
                    : t('theme_use', 'Use theme');
                const deleteLabel = card.querySelector('.theme-delete-button span:last-child');
                if (deleteLabel) deleteLabel.textContent = t('theme_delete', 'Delete');
            }
            updateFilter();
        };
        const openImportButton = document.getElementById('open-theme-import');
        const importForm = document.getElementById('theme-import-form');
        const importName = document.getElementById('theme-import-name');
        const importDescription = document.getElementById('theme-import-description');
        const importColor = document.getElementById('theme-import-color');
        const importColorValue = document.getElementById('theme-import-color-value');
        const importSoulValue = document.getElementById('theme-import-soul-value');
        const importIconPreview = document.getElementById('theme-import-icon-preview');
        const includeMusic = document.getElementById('theme-import-include-music');
        const cancelImportButton = document.getElementById('cancel-theme-import');
        const createThemeButton = document.getElementById('create-theme');
        const importStatus = document.getElementById('theme-import-status');
        let iconPreviewRequest = 0;

        elisten(window, 'deltamod-language-change', updateDynamicTranslations);
        updateDynamicTranslations();

        const colorToHex = color => `#${color
            .map(channel => channel.toString(16).padStart(2, '0'))
            .join('')}`.toUpperCase();

        const updateIconPreview = async () => {
            const request = ++iconPreviewRequest;
            const accent = window.ThemeSprites.parseThemeColor(importColor.value) || [205, 68, 81];
            const soul = window.ThemeSprites.canonicalSoulColor(accent);
            const soulHex = colorToHex(soul);
            importColorValue.textContent = importColor.value.toUpperCase();
            importSoulValue.textContent = soulHex;
            const icon = await window.ThemeSprites.renderAppIcon(importColor.value, soulHex);
            if (request === iconPreviewRequest) importIconPreview.src = icon;
        };

        const closeImportForm = () => {
            importForm.hidden = true;
            openImportButton.hidden = false;
            importStatus.textContent = '';
        };

        openImportButton.addEventListener('click', () => {
            openImportButton.hidden = true;
            importForm.hidden = false;
            importName.focus();
            updateIconPreview().catch(() => {});
        });
        cancelImportButton.addEventListener('click', closeImportForm);
        importColor.addEventListener('input', () => updateIconPreview().catch(() => {}));

        importForm.addEventListener('submit', async event => {
            event.preventDefault();
            const name = importName.value.trim();
            if (!name) {
                importStatus.textContent = t('theme_name_required', 'Enter a name for the theme.');
                importName.focus();
                return;
            }

            createThemeButton.disabled = true;
            cancelImportButton.disabled = true;
            importStatus.textContent = includeMusic.checked
                ? t(
                    'theme_choose_background_music',
                    'Choose a background image, then choose the music file.'
                )
                : t('theme_choose_background', 'Choose a background image.');

            try {
                const accent = window.ThemeSprites.parseThemeColor(importColor.value) || [205, 68, 81];
                const result = await window.deltamodBackend.invoke('importTheme', [{
                    name,
                    description: importDescription.value.trim(),
                    includeMusic: includeMusic.checked,
                    color: importColor.value.toUpperCase(),
                    soulColor: colorToHex(window.ThemeSprites.canonicalSoulColor(accent))
                }]);
                if (!result?.created) {
                    importStatus.textContent = t(
                        'theme_import_canceled',
                        'Import canceled. No theme files were copied.'
                    );
                    return;
                }
                await page('themesel');
            } catch (error) {
                importStatus.textContent = t(
                    'theme_import_failed',
                    'Theme import failed: {0}',
                    error.message || t('theme_unknown_error', 'Unknown error')
                );
            } finally {
                createThemeButton.disabled = false;
                cancelImportButton.disabled = false;
            }
        });

        genbtnstyles();
    })();

    const cancelPendingCharaStart = () => {
        const token = pendingCharaToken;
        if (!token || !charaSessionGate.cancel(token)) return false;
        pendingCharaToken = null;
        charaDetected = false;
        return true;
    };

    function createElement(tagName, className, text) {
        const element = document.createElement(tagName);
        if (className) element.className = className;
        if (text !== undefined) element.textContent = text;
        return element;
    }

    async function startCharaEasterEgg() {
        if (charaDetected) return;
        charaDetected = true;
        await charaUnlockReady;
        const sessionToken = charaSessionGate.begin();
        if (!sessionToken) {
            charaDetected = false;
            return;
        }
        pendingCharaToken = sessionToken;
        const previouslyFocusedElement = document.activeElement;
        const prefersReducedMotion = Boolean(
            window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches
        );
        window._onClosePage = window._onClosePage || [];
        window._onClosePage.push(() => {
            if (!charaSessionGate.cancel(sessionToken)) return;
            if (pendingCharaToken === sessionToken) pendingCharaToken = null;
            charaDetected = false;
        });
        const [musicEnabled, sfxEnabled] = await Promise.all([
            window.deltamodBackend.invoke('getUniqueFlag', ['AUDIO']),
            window.deltamodBackend.invoke('getUniqueFlag', ['SFX'])
        ]).catch(() => [true, true]);
        if (!charaSessionGate.isCurrent(sessionToken)) return;
        if (!charaUnlocked) {
            try {
                await window.deltamodBackend.invoke('setUniqueFlag', [CHARA_UNLOCK_FLAG, true]);
            } catch (error) {
                console.error('Unable to persist the Chara theme unlock:', error);
                charaSessionGate.cancel(sessionToken);
                charaDetected = false;
                pendingCharaToken = null;
                return;
            }
        }
        if (!charaSessionGate.isCurrent(sessionToken)) return;
        charaUnlocked = true;
        charaEncounterActive = true;
        pendingCharaToken = null;

        const themeFilter = document.getElementById('theme-filter');
        if (themeFilter) {
            themeFilter.value = '';
            themeFilter.placeholder = themeFilterPlaceholder();
            themeFilter.dispatchEvent(new Event('input', { bubbles: true }));
        }

        const overlay = createElement('section', 'chara-easter-egg');
        overlay.id = 'chara-easter-egg';
        overlay.tabIndex = -1;
        overlay.setAttribute('role', 'dialog');
        overlay.setAttribute('aria-modal', 'true');
        overlay.setAttribute('aria-label', 'A hidden encounter');
        overlay.dataset.phase = 'dialogue';
        overlay.dataset.reducedMotion = String(prefersReducedMotion);

        const stage = createElement('div', 'chara-stage');
        const portraitFrame = createElement('div', 'chara-portrait-frame');
        const portrait = createElement('img', 'chara-portrait');
        portrait.src = `${CHARA_ASSET_ROOT}/chara-normal.png`;
        portrait.alt = 'Chara';
        portrait.draggable = false;
        portraitFrame.appendChild(portrait);

        const dialogue = createElement('div', 'chara-dialogue');
        const dialogueText = createElement('p', 'chara-dialogue-text');
        const continueIndicator = createElement('span', 'chara-continue-indicator', '▼');
        continueIndicator.setAttribute('aria-hidden', 'true');
        dialogue.append(dialogueText, continueIndicator);

        const dialogueAnnouncer = createElement('span', 'chara-dialogue-announcer');
        dialogueAnnouncer.setAttribute('role', 'status');
        dialogueAnnouncer.setAttribute('aria-live', 'polite');
        dialogueAnnouncer.setAttribute('aria-atomic', 'true');

        const choices = createElement('div', 'chara-choices');
        choices.hidden = true;
        choices.setAttribute('aria-label', 'Choose an answer');
        const proceedButton = createElement('button', 'chara-choice', 'PROCEED');
        proceedButton.type = 'button';
        const goBackButton = createElement('button', 'chara-choice', 'GO BACK');
        goBackButton.type = 'button';
        choices.append(proceedButton, goBackButton);
        stage.append(portraitFrame, dialogue, choices);

        const slash = createElement('img', 'chara-strike');
        slash.src = `${CHARA_ASSET_ROOT}/strike-0.png`;
        slash.alt = '';
        slash.draggable = false;
        slash.hidden = true;

        const numberScreen = createElement('div', 'chara-number-screen');
        numberScreen.setAttribute('aria-hidden', 'true');
        overlay.append(stage, slash, numberScreen, dialogueAnnouncer);
        document.body.appendChild(overlay);
        document.body.classList.add('chara-sequence-active');

        const fallenLoop = createCrossfadedAudioLoop(
            `${CHARA_ASSET_ROOT}/fallen-child.ogg`,
            0.55
        );
        const laughAudio = new Audio(`${CHARA_ASSET_ROOT}/chara-laugh.ogg`);
        const slashAudio = new Audio(`${CHARA_ASSET_ROOT}/slash.wav`);
        const damageAudio = new Audio(`${CHARA_ASSET_ROOT}/damage.wav`);
        laughAudio.volume = 0.62;
        slashAudio.volume = 0.74;
        damageAudio.volume = 0.74;

        const menuAudioWasPlaying = typeof audio !== 'undefined' && audio && !audio.paused;
        if (typeof audio !== 'undefined' && audio) audio.pause();
        if (musicEnabled) fallenLoop.play().catch(() => {});

        let phase = 'dialogue';
        let dialogueIndex = 0;
        let typingTimer = null;
        let typingText = '';
        let typingPosition = 0;
        let afterTyping = null;
        let laughFrameTimer = null;
        let shakeRequestGeneration = 0;
        let disposed = false;
        const timeouts = new Set();

        const later = (callback, delay) => {
            const timeout = setTimeout(() => {
                timeouts.delete(timeout);
                if (!disposed) callback();
            }, delay);
            timeouts.add(timeout);
            return timeout;
        };

        const setWindowShake = phase => {
            const requestGeneration = phase === 'stop'
                ? ++shakeRequestGeneration
                : shakeRequestGeneration;
            if (prefersReducedMotion && phase !== 'stop') {
                overlay.classList.remove('is-window-shake-fallback');
                return;
            }
            const shakeWindow = window.communityAPI?.app?.shakeForEasterEgg;
            if (typeof shakeWindow !== 'function') {
                overlay.classList.toggle('is-window-shake-fallback', phase !== 'stop');
                return;
            }
            shakeWindow(phase)
                .then(result => {
                    if (!charaSessionGate.isCurrent(sessionToken)) return;
                    if (requestGeneration !== shakeRequestGeneration) {
                        if (phase !== 'stop') shakeWindow('stop').catch(() => {});
                        return;
                    }
                    overlay.classList.toggle(
                        'is-window-shake-fallback',
                        phase !== 'stop' && !result.native
                    );
                })
                .catch(() => {
                    if (!charaSessionGate.isCurrent(sessionToken)) return;
                    overlay.classList.toggle('is-window-shake-fallback', phase !== 'stop');
                });
        };

        const resetSound = sound => {
            sound.pause();
            sound.currentTime = 0;
        };

        const cleanup = ({
            restoreMenuAudio = true,
            restoreFocus = true,
            refreshUnlockedTheme = false
        } = {}) => {
            if (disposed) return;
            disposed = true;
            clearInterval(typingTimer);
            clearInterval(laughFrameTimer);
            timeouts.forEach(clearTimeout);
            timeouts.clear();
            fallenLoop.stop();
            [laughAudio, slashAudio, damageAudio].forEach(resetSound);
            setWindowShake('stop');
            overlay.remove();
            document.body.classList.remove('chara-sequence-active');
            charaEncounterActive = false;
            refreshThemeFilterPlaceholder();
            if (restoreMenuAudio && menuAudioWasPlaying && typeof audio !== 'undefined' && audio?.src) {
                audio.play().catch(() => {});
            }
            charaSessionGate.cancel(sessionToken);
            charaDetected = false;
            charaBuffer = '';
            if (restoreFocus) {
                const focusTarget = previouslyFocusedElement?.isConnected
                    ? previouslyFocusedElement
                    : themeFilter;
                focusTarget?.focus?.();
            }
            if (refreshUnlockedTheme) page('themesel').catch(() => {});
        };
        window._onClosePage = window._onClosePage || [];
        window._onClosePage.push(() => cleanup({ restoreFocus: false }));

        const presentCompletedLine = () => {
            dialogueText.textContent = typingText;
            dialogueAnnouncer.textContent = typingText;
            continueIndicator.classList.add('is-ready');
            const callback = afterTyping;
            afterTyping = null;
            callback?.();
        };

        const finishTyping = () => {
            if (!typingTimer) return false;
            clearInterval(typingTimer);
            typingTimer = null;
            presentCompletedLine();
            return true;
        };

        const typeLine = (text, onComplete = null) => {
            clearInterval(typingTimer);
            typingText = text;
            typingPosition = 0;
            afterTyping = onComplete;
            dialogueText.textContent = '';
            dialogueAnnouncer.textContent = '';
            continueIndicator.classList.remove('is-ready');
            if (prefersReducedMotion) {
                presentCompletedLine();
                return;
            }
            typingTimer = setInterval(() => {
                typingPosition += 1;
                dialogueText.textContent = typingText.slice(0, typingPosition);
                if (typingPosition >= typingText.length) finishTyping();
            }, 38);
        };

        const showChoices = () => {
            phase = 'choice';
            overlay.dataset.phase = phase;
            choices.hidden = false;
            continueIndicator.classList.remove('is-ready');
            dialogueAnnouncer.textContent = 'Choose an answer.';
            proceedButton.focus();
        };

        const advanceDialogue = () => {
            if (phase !== 'dialogue') return;
            if (finishTyping()) return;
            if (dialogueIndex < CHARA_DIALOGUE.length - 1) {
                dialogueIndex += 1;
                typeLine(CHARA_DIALOGUE[dialogueIndex]);
            } else {
                showChoices();
            }
        };

        const showNumberScreen = () => {
            phase = 'numbers';
            overlay.dataset.phase = phase;
            stage.hidden = true;
            slash.hidden = true;
            numberScreen.classList.add('is-visible');
            setWindowShake('numbers');
            if (sfxEnabled) {
                damageAudio.currentTime = 0;
                damageAudio.play().catch(() => {});
            }
            later(async () => {
                try {
                    await window.communityAPI.app.quitForEasterEgg();
                } catch {
                    window.close();
                }
            }, prefersReducedMotion ? 1100 : 2800);
        };

        const playSlash = () => {
            if (phase !== 'scare') return;
            phase = 'slash';
            overlay.dataset.phase = phase;
            clearInterval(laughFrameTimer);
            overlay.classList.remove('is-red-flashing');
            stage.hidden = true;
            slash.hidden = false;
            setWindowShake('slash');
            if (sfxEnabled) {
                slashAudio.currentTime = 0;
                slashAudio.play().catch(() => {});
            }
            if (prefersReducedMotion) {
                slash.src = `${CHARA_ASSET_ROOT}/strike-3.png`;
                later(showNumberScreen, 420);
                return;
            }
            let strikeFrame = 0;
            const nextStrike = () => {
                slash.src = `${CHARA_ASSET_ROOT}/strike-${strikeFrame}.png`;
                strikeFrame += 1;
                if (strikeFrame <= 5) {
                    later(nextStrike, 250);
                } else {
                    later(showNumberScreen, 300);
                }
            };
            nextStrike();
        };

        const playScare = () => {
            phase = 'scare';
            overlay.dataset.phase = phase;
            dialogue.hidden = true;
            portrait.src = `${CHARA_ASSET_ROOT}/chara-weird.png`;
            portraitFrame.classList.add('is-weird');
            fallenLoop.stop();
            if (sfxEnabled) {
                laughAudio.currentTime = 0;
                laughAudio.play().catch(() => {});
            }
            if (!prefersReducedMotion) overlay.classList.add('is-red-flashing');
            later(() => {
                portraitFrame.classList.remove('is-weird');
                portraitFrame.classList.add('is-lunging');
                if (prefersReducedMotion) {
                    portrait.src = `${CHARA_ASSET_ROOT}/chara-laugh-1.png`;
                    later(playSlash, 420);
                    return;
                }
                let laughFrame = 0;
                const renderLaughFrame = () => {
                    portrait.src = `${CHARA_ASSET_ROOT}/chara-laugh-${laughFrame}.png`;
                    laughFrame = (laughFrame + 1) % 3;
                };
                renderLaughFrame();
                laughFrameTimer = setInterval(renderLaughFrame, 120);
                portraitFrame.addEventListener(
                    'animationend',
                    event => {
                        if (event.animationName === 'chara-lunge') playSlash();
                    },
                    { once: true }
                );
                later(playSlash, 2800);
            }, prefersReducedMotion ? 180 : 520);
        };

        proceedButton.addEventListener('click', () => {
            if (phase !== 'choice') return;
            phase = 'proceed';
            overlay.dataset.phase = phase;
            choices.hidden = true;
            overlay.focus();
            typeLine('VERY WELL.\nLET US BEGIN.');
            later(async () => {
                cleanup({ restoreMenuAudio: false, restoreFocus: false });
                try {
                    await invoke('setTheme', ['chara']);
                    await themeRefresh(false);
                    await page('main');
                } catch (error) {
                    console.error('Unable to activate the unlocked Chara theme:', error);
                    openAudio();
                }
            }, prefersReducedMotion ? 800 : 1500);
        });

        goBackButton.addEventListener('click', () => {
            if (phase !== 'choice') return;
            phase = 'refusal';
            overlay.dataset.phase = phase;
            choices.hidden = true;
            overlay.focus();
            typeLine('SINCE WHEN WERE YOU\nTHE ONE IN CONTROL?', () => {
                later(playScare, prefersReducedMotion ? 240 : 620);
            });
        });

        overlay.addEventListener('pointerup', event => {
            if (event.target.closest('.chara-choice')) return;
            advanceDialogue();
        });
        overlay.addEventListener('keydown', event => {
            if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                cleanup({ refreshUnlockedTheme: true });
                return;
            }
            if (event.key === 'Tab') {
                event.preventDefault();
                const focusCycle = phase === 'choice'
                    ? [proceedButton, goBackButton]
                    : [overlay];
                const currentIndex = focusCycle.indexOf(document.activeElement);
                const direction = event.shiftKey ? -1 : 1;
                const nextIndex = currentIndex === -1
                    ? 0
                    : (currentIndex + direction + focusCycle.length) % focusCycle.length;
                focusCycle[nextIndex].focus();
                return;
            }
            if (phase === 'choice') {
                if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                    event.preventDefault();
                    (document.activeElement === proceedButton ? goBackButton : proceedButton).focus();
                }
                return;
            }
            if ((event.key === 'Enter' || event.key === ' ') && phase === 'dialogue') {
                event.preventDefault();
                advanceDialogue();
            }
        });

        overlay.focus();
        typeLine(CHARA_DIALOGUE[0]);
    }

    const handleSecretKeydown = event => {
        if (event.ctrlKey || event.altKey || event.metaKey) return;
        if (event.key.length === 1) {
            const key = event.key.toLowerCase();
            charaBuffer = (charaBuffer + key).slice(-5);
            if (!charaDetected && charaBuffer === 'chara') {
                startCharaEasterEgg();
            }
        } else if (['Backspace', 'Delete', 'Escape'].includes(event.key)) {
            charaBuffer = '';
            if (event.key === 'Escape') cancelPendingCharaStart();
        }
    };
    elisten(document, 'keydown', handleSecretKeydown);
})();
