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
        stallFallbacks: 0
    };

    function warn(message, error) {
        root.console?.warn?.(message, error || '');
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
        auto: 'Uses the static theme poster and separate audio while keeping normal visual effects. Recommended on Linux.',
        performance: 'Uses the static theme poster and separate audio with reduced blur and decorative animation.',
        quality: 'Uses native streamed theme video and full visual effects. This can use substantially more CPU on WebKitGTK.'
    });

    function injectLinuxModeControl() {
        const doc = root.document;
        const tableBody = doc?.querySelector?.('#options')
            || doc?.querySelector?.('.options-page tbody');
        if (!tableBody || doc.getElementById?.('SELECT-LINUX-PERFORMANCE-MODE')) return false;

        const row = doc.createElement('tr');
        row.dataset.deltamodLinuxSetting = 'rendering-mode';

        const label = doc.createElement('td');
        const title = doc.createElement('span');
        title.className = 'setting-title';
        title.textContent = 'Linux rendering mode';
        const description = doc.createElement('small');
        description.className = 'calibri';
        description.style.display = 'block';
        description.style.marginTop = '4px';

        label.append(title, doc.createElement('br'), description);

        const action = doc.createElement('td');
        action.className = 'setting-control-cell center';
        const control = doc.createElement('div');
        control.className = 'setting-control';
        const select = doc.createElement('select');
        select.id = 'SELECT-LINUX-PERFORMANCE-MODE';
        select.setAttribute('aria-label', 'Linux rendering mode');

        for (const [value, text] of [
            ['auto', 'Auto (recommended)'],
            ['performance', 'Performance'],
            ['quality', 'Quality']
        ]) {
            const option = doc.createElement('option');
            option.value = value;
            option.textContent = text;
            select.appendChild(option);
        }

        const refreshDescription = () => {
            const current = compat.getMode();
            select.value = current;
            description.textContent = modeCopy[current] || modeCopy.auto;
        };
        refreshDescription();

        select.addEventListener('change', () => {
            try {
                compat.setMode(select.value);
                refreshDescription();
            } catch (error) {
                warn('Unable to change Linux rendering mode.', error);
                refreshDescription();
            }
        });

        control.appendChild(select);
        action.appendChild(control);
        row.append(label, action);
        tableBody.appendChild(row);
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
    installVideoHealthMonitor();

    root.DeltamodLinuxRuntimePolish = Object.freeze({
        refreshOptions: patchOptionsCategory,
        playRewind: playManagedRewind,
        snapshot: () => Object.freeze({ ...diagnostics })
    });

    return 'active';
});
