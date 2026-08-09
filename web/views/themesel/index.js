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
    const DEFAULT_THEME_FILTER_PLACEHOLDER = 'Name, description, or music';

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

    function safeThemeBackground(value) {
        const fileName = String(value || '');
        return /^[a-z0-9._-]+$/i.test(fileName)
            ? `url("${window.deltamodBackend.assetUrl('theme', `img/${fileName}`)}")`
            : '';
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
        const themes = (await window.deltamodBackend.invoke('getThemes', [])).sort((a, b) => {
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
        const currentTheme = await window.deltamodBackend.invoke('getTheme', []);
        const themeGrid = document.getElementById('themes');
        const filterInput = document.getElementById('theme-filter');
        const countLabel = document.getElementById('theme-count');
        const emptyState = document.getElementById('theme-empty');
        const themeCards = [];

        for (const theme of themes) {
            if (theme.hiddenByDefault && theme.id !== currentTheme) continue;

            const card = document.createElement('article');
            card.className = 'theme-card';
            card.dataset.search = [
                theme.name,
                theme.description,
                theme.musicTrack,
                ...(Array.isArray(theme.credits)
                    ? theme.credits.flatMap(credit => [credit.role, credit.name])
                    : []),
                theme.builtIn ? 'built-in' : 'custom'
            ].join(' ').toLocaleLowerCase();
            if (theme.id === currentTheme) card.classList.add('is-current');

            const preview = document.createElement('div');
            preview.className = 'theme-card-preview';
            preview.style.backgroundImage = safeThemeBackground(theme.background);
            preview.setAttribute('role', 'img');
            preview.setAttribute('aria-label', `${theme.name} background preview`);
            card.appendChild(preview);

            const content = document.createElement('div');
            content.className = 'theme-card-content';

            const heading = document.createElement('div');
            heading.className = 'theme-card-heading';
            const accent = document.createElement('span');
            accent.className = 'theme-accent';
            accent.style.backgroundColor = theme.color;
            accent.title = theme.soulColor
                ? `UI accent: ${theme.color}; SOUL color: ${theme.soulColor}`
                : `Theme accent: ${theme.color}`;
            const name = document.createElement('h2');
            name.textContent = theme.name;
            heading.append(accent, name);

            const source = document.createElement('span');
            source.className = 'theme-source';
            source.textContent = theme.builtIn ? 'Built-in' : 'Custom';
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
                summary.textContent = 'Credits';
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
                name.title = 'Click to edit the theme name';
                description.title = 'Click to edit the description';

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
            selectButton.className = 'theme-select-button';
            selectButton.textContent = theme.id === currentTheme ? 'In use' : 'Use theme';
            selectButton.disabled = theme.id === currentTheme;
            selectButton.setAttribute('aria-pressed', String(theme.id === currentTheme));
            selectButton.addEventListener('click', async () => {
                await window.deltamodBackend.invoke('setTheme', [theme.id]);
                await themeRefresh(true);
            });
            actions.appendChild(selectButton);

            if (!theme.builtIn) {
                const deleteButton = document.createElement('button');
                deleteButton.type = 'button';
                deleteButton.className = 'theme-delete-button';
                deleteButton.appendChild(staticIcon('delete', '19px'));
                const deleteLabel = document.createElement('span');
                deleteLabel.textContent = 'Delete';
                deleteButton.appendChild(deleteLabel);
                deleteButton.addEventListener('click', async () => {
                    if (!window.confirm(`Delete "${theme.name}"? This cannot be undone.`)) return;
                    if (theme.id === currentTheme) {
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
                const matches = !query || card.dataset.search.includes(query);
                card.hidden = !matches;
                if (matches) visible += 1;
            }
            countLabel.textContent = `${visible} of ${themeCards.length} themes`;
            emptyState.hidden = visible !== 0;
        };
        filterInput.addEventListener('input', updateFilter);
        updateFilter();

        const openImportButton = document.getElementById('open-theme-import');
        const importForm = document.getElementById('theme-import-form');
        const importName = document.getElementById('theme-import-name');
        const importDescription = document.getElementById('theme-import-description');
        const includeMusic = document.getElementById('theme-import-include-music');
        const cancelImportButton = document.getElementById('cancel-theme-import');
        const createThemeButton = document.getElementById('create-theme');
        const importStatus = document.getElementById('theme-import-status');

        const closeImportForm = () => {
            importForm.hidden = true;
            openImportButton.hidden = false;
            importStatus.textContent = '';
        };

        openImportButton.addEventListener('click', () => {
            openImportButton.hidden = true;
            importForm.hidden = false;
            importName.focus();
        });
        cancelImportButton.addEventListener('click', closeImportForm);

        importForm.addEventListener('submit', async event => {
            event.preventDefault();
            const name = importName.value.trim();
            if (!name) {
                importStatus.textContent = 'Enter a name for the theme.';
                importName.focus();
                return;
            }

            createThemeButton.disabled = true;
            cancelImportButton.disabled = true;
            importStatus.textContent = includeMusic.checked
                ? 'Choose a background image, then choose the music file.'
                : 'Choose a background image.';

            try {
                const result = await window.deltamodBackend.invoke('importTheme', [{
                    name,
                    description: importDescription.value.trim(),
                    includeMusic: includeMusic.checked
                }]);
                if (!result?.created) {
                    importStatus.textContent = 'Import canceled. No theme files were copied.';
                    return;
                }
                await page('themesel');
            } catch (error) {
                importStatus.textContent = `Theme import failed: ${error.message || 'Unknown error'}`;
            } finally {
                createThemeButton.disabled = false;
                cancelImportButton.disabled = false;
            }
        });

        genbtnstyles();
    })();

    let charaBuffer = '';
    let charaDetected = false;

    function createElement(tagName, className, text) {
        const element = document.createElement(tagName);
        if (className) element.className = className;
        if (text !== undefined) element.textContent = text;
        return element;
    }

    async function startCharaEasterEgg() {
        if (charaDetected) return;
        charaDetected = true;
        const [musicEnabled, sfxEnabled] = await Promise.all([
            window.deltamodBackend.invoke('getUniqueFlag', ['AUDIO']),
            window.deltamodBackend.invoke('getUniqueFlag', ['SFX'])
        ]).catch(() => [true, true]);

        const themeFilter = document.getElementById('theme-filter');
        if (themeFilter) {
            themeFilter.value = '';
            themeFilter.dispatchEvent(new Event('input', { bubbles: true }));
        }

        const overlay = createElement('section', 'chara-easter-egg');
        overlay.id = 'chara-easter-egg';
        overlay.tabIndex = -1;
        overlay.setAttribute('role', 'dialog');
        overlay.setAttribute('aria-modal', 'true');
        overlay.setAttribute('aria-label', 'A hidden encounter');
        overlay.dataset.phase = 'dialogue';

        const stage = createElement('div', 'chara-stage');
        const portraitFrame = createElement('div', 'chara-portrait-frame');
        const portrait = createElement('img', 'chara-portrait');
        portrait.src = `${CHARA_ASSET_ROOT}/chara-normal.png`;
        portrait.alt = 'Chara';
        portrait.draggable = false;
        portraitFrame.appendChild(portrait);

        const dialogue = createElement('div', 'chara-dialogue');
        dialogue.setAttribute('aria-live', 'polite');
        const dialogueText = createElement('p', 'chara-dialogue-text');
        const continueIndicator = createElement('span', 'chara-continue-indicator', '▼');
        continueIndicator.setAttribute('aria-hidden', 'true');
        dialogue.append(dialogueText, continueIndicator);

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
        overlay.append(stage, slash, numberScreen);
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
            const shakeWindow = window.communityAPI?.app?.shakeForEasterEgg;
            if (typeof shakeWindow !== 'function') {
                overlay.classList.toggle('is-window-shake-fallback', phase !== 'stop');
                return;
            }
            shakeWindow(phase)
                .then(result => {
                    overlay.classList.toggle(
                        'is-window-shake-fallback',
                        phase !== 'stop' && !result.native
                    );
                })
                .catch(() => {
                    overlay.classList.toggle('is-window-shake-fallback', phase !== 'stop');
                });
        };

        const resetSound = sound => {
            sound.pause();
            sound.currentTime = 0;
        };

        const cleanup = ({ restoreMenuAudio = true } = {}) => {
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
            if (themeFilter) {
                themeFilter.placeholder = DEFAULT_THEME_FILTER_PLACEHOLDER;
            }
            if (restoreMenuAudio && menuAudioWasPlaying && typeof audio !== 'undefined' && audio?.src) {
                audio.play().catch(() => {});
            }
        };
        window._onClosePage = window._onClosePage || [];
        window._onClosePage.push(() => cleanup());

        const finishTyping = () => {
            if (!typingTimer) return false;
            clearInterval(typingTimer);
            typingTimer = null;
            dialogueText.textContent = typingText;
            continueIndicator.classList.add('is-ready');
            const callback = afterTyping;
            afterTyping = null;
            callback?.();
            return true;
        };

        const typeLine = (text, onComplete = null) => {
            clearInterval(typingTimer);
            typingText = text;
            typingPosition = 0;
            afterTyping = onComplete;
            dialogueText.textContent = '';
            continueIndicator.classList.remove('is-ready');
            typingTimer = setInterval(() => {
                typingPosition += 1;
                dialogueText.textContent = typingText.slice(0, typingPosition);
                if (typingPosition >= typingText.length) finishTyping();
            }, 38);
        };

        const showChoices = () => {
            phase = 'choice';
            choices.hidden = false;
            continueIndicator.classList.remove('is-ready');
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
            }, 2800);
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
            overlay.classList.add('is-red-flashing');
            later(() => {
                portraitFrame.classList.remove('is-weird');
                portraitFrame.classList.add('is-lunging');
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
            }, 520);
        };

        proceedButton.addEventListener('click', () => {
            if (phase !== 'choice') return;
            phase = 'proceed';
            overlay.dataset.phase = phase;
            choices.hidden = true;
            typeLine('VERY WELL.\nLET US BEGIN.');
            later(async () => {
                cleanup({ restoreMenuAudio: false });
                try {
                    await invoke('setTheme', ['chara']);
                    await themeRefresh(false);
                    await page('main');
                } catch (error) {
                    console.error('Unable to activate the Chara theme:', error);
                    openAudio();
                }
            }, 1500);
        });

        goBackButton.addEventListener('click', () => {
            if (phase !== 'choice') return;
            phase = 'refusal';
            overlay.dataset.phase = phase;
            choices.hidden = true;
            typeLine('SINCE WHEN WERE YOU\nTHE ONE IN CONTROL?', () => {
                later(playScare, 620);
            });
        });

        overlay.addEventListener('pointerup', event => {
            if (event.target.closest('.chara-choice')) return;
            advanceDialogue();
        });
        overlay.addEventListener('keydown', event => {
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
        }
    };
    elisten(document, 'keydown', handleSecretKeydown);
})();
