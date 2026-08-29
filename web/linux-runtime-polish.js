(function initializeLinuxRuntimePolish(root, factory) {
    if (typeof module === 'object' && module.exports) {
        module.exports = factory;
        return;
    }
    factory(root);
})(typeof window === 'undefined' ? globalThis : window, function installLinuxRuntimePolish(root) {
    'use strict';

    const compat = root.DeltamodLinuxCompat;
    if (!compat?.isLinuxTauri) return 'inactive';

    const diagnostics = {
        manualLoopRemoved: false,
        rewindPlays: 0,
        rewindErrors: 0,
        videoStalls: 0,
        droppedFrameFallbacks: 0,
        stallFallbacks: 0,
        qualityVideoFallbackReason: null
    };

    function warn(message, error) {
        root.console?.warn?.(message, error || '');
    }

    function localize(key, fallback, ...args) {
        return root.Localization?.t?.(key, fallback, ...args) || fallback;
    }

    function polishMenuAudio() {
        const menuAudio = root.audio;
        if (!menuAudio) return;
        if (typeof root.loopMenuAudio === 'function' && menuAudio.removeEventListener) {
            menuAudio.removeEventListener('timeupdate', root.loopMenuAudio);
            diagnostics.manualLoopRemoved = true;
        }
        menuAudio.loop = true;
    }

    const NativeAudio = root.Audio;
    let rewindPlayer = null;

    function getRewindPlayer() {
        if (rewindPlayer || typeof NativeAudio !== 'function') return rewindPlayer;
        rewindPlayer = new NativeAudio();
        rewindPlayer.preload = 'auto';
        rewindPlayer.src = 'audio/rew.mp3';
        if (typeof rewindPlayer.setAttribute === 'function') {
            rewindPlayer.setAttribute('data-deltamod-retain-media-blob', 'true');
        } else if (rewindPlayer.dataset) {
            rewindPlayer.dataset.deltamodRetainMediaBlob = 'true';
        }
        return rewindPlayer;
    }

    async function playManagedRewind() {
        try {
            const enabled = await root.deltamodBackend?.invoke?.('getUniqueFlag', ['SFX']);
            if (enabled === false) return false;
            const player = getRewindPlayer();
            if (!player) return false;
            player.pause?.();
            try {
                player.currentTime = 0;
            } catch (error) {
                warn('Unable to reset Linux rewind SFX position.', error);
            }
            player.playbackRate = 1;
            await player.play();
            diagnostics.rewindPlays += 1;
            return true;
        } catch (error) {
            diagnostics.rewindErrors += 1;
            warn('Unable to play Linux rewind SFX.', error);
            return false;
        }
    }

    if (typeof root.rew === 'function') {
        root.rew = playManagedRewind;
    }

    const modeCopy = Object.freeze({
        auto: Object.freeze({
            labelKey: 'linux_rendering_auto',
            label: 'Auto (recommended)',
            descriptionKey: 'linux_rendering_auto_desc',
            description: 'Uses the static theme poster and separate audio with Linux-safe visual effects. Recommended on Linux.'
        }),
        performance: Object.freeze({
            labelKey: 'linux_rendering_performance',
            label: 'Performance',
            descriptionKey: 'linux_rendering_performance_desc',
            description: 'Uses the static theme poster and separate audio with reduced blur and decorative animation.'
        }),
        quality: Object.freeze({
            labelKey: 'linux_rendering_quality',
            label: 'Quality',
            descriptionKey: 'linux_rendering_quality_desc',
            description: 'Uses a buffered theme video for stable WebKitGTK playback and full visual effects. This can use substantially more CPU and memory.'
        })
    });

    function currentQualityFallbackReason() {
        if (compat.getMode() !== 'quality') return null;
        if (root.themeVideoFallbackActive === false) {
            diagnostics.qualityVideoFallbackReason = null;
        }
        return diagnostics.qualityVideoFallbackReason;
    }

    function refreshLinuxModeControl() {
        const doc = root.document;
        const select = doc?.getElementById?.('SELECT-LINUX-PERFORMANCE-MODE');
        const title = doc?.getElementById?.('DELTAMOD-LINUX-RENDERING-TITLE');
        const description = doc?.getElementById?.('DELTAMOD-LINUX-RENDERING-DESCRIPTION');
        const status = doc?.getElementById?.('DELTAMOD-LINUX-RENDERING-STATUS');
        if (!select || !title || !description || !status) return false;

        const current = compat.getMode();
        const config = modeCopy[current] || modeCopy.auto;
        select.value = current;
        title.textContent = localize('linux_rendering_mode', 'Linux rendering mode');
        select.setAttribute(
            'aria-label',
            localize('linux_rendering_mode_aria', 'Linux rendering mode')
        );
        description.textContent = localize(config.descriptionKey, config.description);

        for (const option of select.children || []) {
            const optionConfig = modeCopy[option.value];
            if (optionConfig) {
                option.textContent = localize(optionConfig.labelKey, optionConfig.label);
            }
        }

        const fallbackReason = currentQualityFallbackReason();
        status.hidden = !fallbackReason;
        status.textContent = fallbackReason
            ? localize(
                'linux_rendering_fallback_active',
                'Video fallback active for this theme: {0}',
                fallbackReason
            )
            : '';
        return true;
    }

    function recordQualityFallback(reason) {
        if (compat.getMode() !== 'quality') return;
        diagnostics.qualityVideoFallbackReason = String(reason || 'playback fallback');
        refreshLinuxModeControl();
    }

    function installThemeVideoFallbackObserver() {
        const nativeFallback = root.fallBackFromThemeVideo;
        if (typeof nativeFallback !== 'function' || nativeFallback.__deltamodLinuxFallbackObserved) {
            return;
        }
        const observedFallback = function observedLinuxThemeVideoFallback(video, background, reason, ...args) {
            const qualityAtCall = compat.getMode() === 'quality';
            return Promise.resolve(nativeFallback.call(this, video, background, reason, ...args))
                .then(result => {
                    if (qualityAtCall) recordQualityFallback(reason);
                    return result;
                });
        };
        observedFallback.__deltamodLinuxFallbackObserved = true;
        root.fallBackFromThemeVideo = observedFallback;
    }

    function injectLinuxModeControl() {
        const doc = root.document;
        const tableBody = doc?.querySelector?.('#options')
            || doc?.querySelector?.('.options-page tbody');
        if (!tableBody) return false;
        if (doc.getElementById?.('SELECT-LINUX-PERFORMANCE-MODE')) {
            refreshLinuxModeControl();
            return false;
        }

        const row = doc.createElement('tr');
        row.dataset.deltamodLinuxSetting = 'rendering-mode';

        const label = doc.createElement('td');
        const title = doc.createElement('span');
        title.id = 'DELTAMOD-LINUX-RENDERING-TITLE';
        title.className = 'setting-title';
        const description = doc.createElement('small');
        description.id = 'DELTAMOD-LINUX-RENDERING-DESCRIPTION';
        description.className = 'calibri';
        description.style.display = 'block';
        description.style.marginTop = '4px';
        const status = doc.createElement('small');
        status.id = 'DELTAMOD-LINUX-RENDERING-STATUS';
        status.className = 'calibri';
        status.style.display = 'block';
        status.style.marginTop = '4px';
        status.style.color = 'var(--theme-color)';
        status.hidden = true;

        label.append(title, doc.createElement('br'), description, status);

        const action = doc.createElement('td');
        action.className = 'setting-control-cell center';
        const control = doc.createElement('div');
        control.className = 'setting-control';
        const select = doc.createElement('select');
        select.id = 'SELECT-LINUX-PERFORMANCE-MODE';

        for (const value of ['auto', 'performance', 'quality']) {
            const option = doc.createElement('option');
            option.value = value;
            select.appendChild(option);
        }

        select.addEventListener('change', () => {
            try {
                diagnostics.qualityVideoFallbackReason = null;
                compat.setMode(select.value);
                refreshLinuxModeControl();
            } catch (error) {
                warn('Unable to change Linux rendering mode.', error);
                refreshLinuxModeControl();
            }
        });

        control.appendChild(select);
        action.appendChild(control);
        row.append(label, action);
        tableBody.appendChild(row);
        refreshLinuxModeControl();
        return true;
    }

    function patchOptionsCategory() {
        const stack = root.currentPageStack;
        if (!stack || typeof stack.cat !== 'function' || stack.cat.__deltamodLinuxPolished) {
            if (root.document?.querySelector?.('#b_ui.selected')) injectLinuxModeControl();
            return;
        }

        const nativeCat = stack.cat;
        const wrappedCat = async function wrappedLinuxOptionsCategory(category, ...args) {
            const result = await nativeCat.call(this, category, ...args);
            if (category === 'ui') injectLinuxModeControl();
            return result;
        };
        wrappedCat.__deltamodLinuxPolished = true;
        stack.cat = wrappedCat;

        if (root.document?.querySelector?.('#b_ui.selected')) injectLinuxModeControl();
    }

    if (typeof root.page === 'function' && !root.page.__deltamodLinuxPolished) {
        const nativePage = root.page;
        const wrappedPage = function wrappedLinuxPage(...args) {
            return Promise.resolve(nativePage.apply(this, args)).then(result => {
                patchOptionsCategory();
                return result;
            });
        };
        wrappedPage.__deltamodLinuxPolished = true;
        root.page = wrappedPage;
    }
    patchOptionsCategory();

    function installVideoHealthMonitor() {
        const video = root.document?.getElementById?.('theme-background-video');
        if (!video?.addEventListener) return;

        const stallTimes = [];
        let lastStallAt = -Infinity;
        let lastSource = '';
        let previousTotalFrames = 0;
        let previousDroppedFrames = 0;
        const now = () => root.performance?.now?.() ?? Date.now();

        function resetFrameBaseline() {
            lastSource = video.dataset?.source || video.currentSrc || video.src || '';
            const quality = video.getVideoPlaybackQuality?.();
            previousTotalFrames = quality?.totalVideoFrames || 0;
            previousDroppedFrames = quality?.droppedVideoFrames || 0;
        }

        function fallback(reason, counter) {
            if (compat.getMode() !== 'quality') return;
            const background = root.document?.querySelector?.('.bg');
            if (!background || typeof root.fallBackFromThemeVideo !== 'function') {
                warn(`Linux theme video is unhealthy (${reason}), but the poster fallback is unavailable.`);
                return;
            }
            diagnostics[counter] += 1;
            Promise.resolve(root.fallBackFromThemeVideo(video, background, reason)).catch(error => {
                warn('Unable to activate Linux theme video fallback.', error);
            });
        }

        function recordStall() {
            if (compat.getMode() !== 'quality' || video.hidden || video.paused) return;
            const current = now();
            if (current - lastStallAt < 750) return;
            lastStallAt = current;
            diagnostics.videoStalls += 1;
            stallTimes.push(current);
            while (stallTimes.length && current - stallTimes[0] > 15000) stallTimes.shift();
            if (stallTimes.length >= 4) {
                stallTimes.length = 0;
                fallback('repeated stalls on Linux WebKitGTK', 'stallFallbacks');
            }
        }

        video.addEventListener('waiting', recordStall);
        video.addEventListener('stalled', recordStall);
        video.addEventListener('playing', () => {
            if (compat.getMode() !== 'quality') return;
            diagnostics.qualityVideoFallbackReason = null;
            refreshLinuxModeControl();
        });
        video.addEventListener('loadedmetadata', resetFrameBaseline);
        video.addEventListener('emptied', () => {
            stallTimes.length = 0;
            resetFrameBaseline();
        });
        resetFrameBaseline();

        const frameTimer = root.setInterval?.(() => {
            if (compat.getMode() !== 'quality' || video.hidden || video.paused) return;
            const source = video.dataset?.source || video.currentSrc || video.src || '';
            if (source !== lastSource) {
                resetFrameBaseline();
                return;
            }
            const quality = video.getVideoPlaybackQuality?.();
            if (!quality) return;
            const totalDelta = quality.totalVideoFrames - previousTotalFrames;
            const droppedDelta = quality.droppedVideoFrames - previousDroppedFrames;
            previousTotalFrames = quality.totalVideoFrames;
            previousDroppedFrames = quality.droppedVideoFrames;
            if (totalDelta >= 60 && droppedDelta / totalDelta >= 0.2) {
                fallback(
                    `excessive dropped frames (${droppedDelta}/${totalDelta}) on Linux WebKitGTK`,
                    'droppedFrameFallbacks'
                );
            }
        }, 5000);

        root.addEventListener?.('pagehide', () => {
            if (frameTimer !== undefined) root.clearInterval?.(frameTimer);
        }, { once: true });
    }

    polishMenuAudio();
    installThemeVideoFallbackObserver();
    installVideoHealthMonitor();
    root.addEventListener?.('deltamod-language-change', refreshLinuxModeControl);
    Promise.resolve(root.Localization?.ready).then(refreshLinuxModeControl).catch(() => {});

    root.DeltamodLinuxRuntimePolish = Object.freeze({
        refreshOptions: patchOptionsCategory,
        refreshModeControl: refreshLinuxModeControl,
        playRewind: playManagedRewind,
        snapshot: () => Object.freeze({ ...diagnostics })
    });

    return 'active';
});
