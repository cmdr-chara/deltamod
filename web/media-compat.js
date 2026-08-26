(function initializeMediaCompatibility(root, factory) {
    if (typeof module === 'object' && module.exports) {
        module.exports = factory;
        return;
    }
    factory(root);
})(typeof window === 'undefined' ? globalThis : window, function installMediaCompatibility(root) {
    'use strict';

    const platformText = `${root.navigator?.platform || ''} ${root.navigator?.userAgent || ''}`;
    const isLinux = /linux/i.test(platformText);
    const isTauri = Boolean(root.__TAURI__?.core);

    if (!isLinux || !isTauri) return 'inactive';

    const modeKey = 'deltamodLinuxMediaMode';
    const supportedModes = new Set(['auto', 'performance', 'quality']);
    const supportedSchemes = new Set(['tauri:', 'themeprot:', 'packet:']);
    const html = root.document?.documentElement;
    const diagnostics = {
        audioBlobLoads: 0,
        videoBlobLoads: 0,
        videoDirectPlays: 0,
        videoBlocks: 0,
        revokedObjectUrls: 0,
        lastError: null
    };

    function readMode() {
        try {
            const stored = root.localStorage?.getItem?.(modeKey);
            return supportedModes.has(stored) ? stored : 'auto';
        } catch (error) {
            diagnostics.lastError = `mode-read: ${error?.message || error}`;
            root.console?.warn?.('Unable to read Linux media mode; using auto.', error);
            return 'auto';
        }
    }

    let mode = readMode();
    const elementStates = new WeakMap();
    const cleanupAttached = new WeakSet();
    const objectUrls = new Set();
    const bridgedVideos = new Set();

    function toggleClass(name, enabled) {
        if (html?.classList?.toggle) {
            html.classList.toggle(name, enabled);
            return;
        }
        if (enabled) html?.classList?.add?.(name);
        else html?.classList?.remove?.(name);
    }

    function applyModeClasses() {
        html?.classList?.add?.('deltamod-linux-webkit');
        toggleClass('deltamod-linux-reduced-effects', mode !== 'quality');
        toggleClass('deltamod-linux-performance', mode === 'performance');
        if (html?.dataset) html.dataset.deltamodLinuxMediaMode = mode;
    }

    function setMode(nextMode) {
        if (!supportedModes.has(nextMode)) {
            throw new TypeError(`Unknown Linux media mode: ${nextMode}`);
        }
        if (nextMode !== 'quality') {
            for (const video of [...bridgedVideos]) {
                const state = elementStates.get(video);
                if (state?.source) {
                    video.pause?.();
                    video.src = state.source;
                }
                releaseElementObjectUrl(video);
            }
        }
        mode = nextMode;
        try {
            root.localStorage?.setItem?.(modeKey, mode);
        } catch (error) {
            diagnostics.lastError = `mode-write: ${error?.message || error}`;
            root.console?.warn?.('Unable to persist Linux media mode.', error);
        }
        applyModeClasses();
        root.setTimeout?.(() => root.applyThemeBackground?.(), 0);
        return mode;
    }

    applyModeClasses();

    const compatApi = Object.freeze({
        isLinuxTauri: true,
        getMode: () => mode,
        setMode,
        usesReducedEffects: () => mode !== 'quality',
        forcesPosterVideo: () => mode === 'performance',
        snapshot: () => Object.freeze({ mode, ...diagnostics })
    });
    root.DeltamodLinuxCompat = compatApi;

    const MediaElement = root.HTMLMediaElement;
    const fetchAsset = root.fetch?.bind(root);
    const Url = root.URL;
    if (!MediaElement?.prototype?.play || !fetchAsset || !Url?.createObjectURL) {
        return 'styles-only';
    }

    const nativePlay = MediaElement.prototype.play;

    function normalizeSource(rawSource) {
        if (!rawSource) return null;
        try {
            return new Url(rawSource, root.location?.href).href;
        } catch {
            return null;
        }
    }

    function needsCompatibilitySource(rawSource) {
        const absolute = normalizeSource(rawSource);
        if (!absolute) return false;
        try {
            return supportedSchemes.has(new Url(absolute).protocol);
        } catch {
            return false;
        }
    }

    function mediaKind(element) {
        const tagName = String(element?.tagName || '').toUpperCase();
        if (tagName === 'VIDEO') return 'video';
        if (tagName === 'AUDIO') return 'audio';
        if (root.HTMLVideoElement && element instanceof root.HTMLVideoElement) return 'video';
        return 'audio';
    }

    function mediaSourceFor(element) {
        const assignedSource = element.src
            || element.getAttribute?.('src')
            || element.currentSrc;

        if (needsCompatibilitySource(assignedSource)) return assignedSource;

        const state = elementStates.get(element);
        const absoluteAssignedSource = normalizeSource(assignedSource);
        if (state?.source && absoluteAssignedSource === state.objectUrl) {
            return state.source;
        }

        const originalSource = element.dataset?.deltamodOriginalMediaSource;
        if (absoluteAssignedSource?.startsWith('blob:') && originalSource) {
            return originalSource;
        }
        return assignedSource;
    }

    function revokeObjectUrl(objectUrl) {
        if (!objectUrl || !objectUrls.has(objectUrl)) return;
        Url.revokeObjectURL?.(objectUrl);
        objectUrls.delete(objectUrl);
        diagnostics.revokedObjectUrls += 1;
    }

    function releaseElementObjectUrl(element, { keepOriginal = false } = {}) {
        const state = elementStates.get(element);
        if (!state) return;
        revokeObjectUrl(state.objectUrl);
        if (state.kind === 'video') bridgedVideos.delete(element);
        elementStates.delete(element);
        if (!keepOriginal && element.dataset) {
            delete element.dataset.deltamodOriginalMediaSource;
        }
    }

    function attachElementCleanup(element) {
        if (cleanupAttached.has(element) || !element?.addEventListener) return;
        cleanupAttached.add(element);
        element.addEventListener('ended', () => {
            if (!element.loop) releaseElementObjectUrl(element, { keepOriginal: true });
        });
    }

    function objectUrlFor(element, source, kind) {
        const absolute = normalizeSource(source);
        if (!absolute) return Promise.reject(new TypeError('Invalid media source.'));

        const existing = elementStates.get(element);
        if (existing?.source === absolute) {
            if (existing.objectUrl) return Promise.resolve(existing.objectUrl);
            if (existing.pending) return existing.pending;
        } else if (existing) {
            releaseElementObjectUrl(element);
        }

        const state = { source: absolute, kind, objectUrl: null, pending: null };
        if (kind === 'video') bridgedVideos.add(element);
        const pending = fetchAsset(absolute)
            .then(response => {
                if (!response.ok) {
                    throw new Error(`Media request failed with HTTP ${response.status}.`);
                }
                return response.blob();
            })
            .then(blob => {
                if (elementStates.get(element) !== state) {
                    throw new Error('Media source changed while the Blob bridge was loading.');
                }
                const objectUrl = Url.createObjectURL(blob);
                objectUrls.add(objectUrl);
                state.objectUrl = objectUrl;
                state.pending = null;
                diagnostics[kind === 'video' ? 'videoBlobLoads' : 'audioBlobLoads'] += 1;
                return objectUrl;
            })
            .catch(error => {
                if (elementStates.get(element) === state) {
                    elementStates.delete(element);
                    if (kind === 'video') bridgedVideos.delete(element);
                }
                diagnostics.lastError = `blob-load: ${error?.message || error}`;
                throw error;
            });

        state.pending = pending;
        elementStates.set(element, state);
        attachElementCleanup(element);
        return pending;
    }

    function isThemeBackgroundVideo(element) {
        return element?.id === 'theme-background-video'
            || element?.classList?.contains?.('theme-background-video');
    }

    function performanceVideoError(source) {
        const error = new Error(`Linux performance mode uses the theme poster instead of video: ${source}`);
        error.name = 'NotSupportedError';
        error.code = 'DELTAMOD_LINUX_PERFORMANCE_VIDEO_DISABLED';
        return error;
    }

    MediaElement.prototype.play = function patchedPlay(...args) {
        const currentState = elementStates.get(this);
        const assignedSource = normalizeSource(this.src || this.getAttribute?.('src') || this.currentSrc);
        if (currentState?.objectUrl && assignedSource
            && assignedSource !== currentState.objectUrl
            && assignedSource !== currentState.source) {
            releaseElementObjectUrl(this);
        }

        const source = mediaSourceFor(this);
        if (!needsCompatibilitySource(source)) {
            return nativePlay.apply(this, args);
        }

        const absolute = normalizeSource(source);
        const kind = mediaKind(this);

        if (kind === 'video' && mode === 'performance' && isThemeBackgroundVideo(this)) {
            diagnostics.videoBlocks += 1;
            const error = performanceVideoError(absolute);
            diagnostics.lastError = error.code;
            root.console?.warn?.(error.message);
            return Promise.reject(error);
        }

        // Auto mode deliberately keeps video on the native WebKit/GStreamer path.
        // If the host cannot decode or stream the custom URI, Deltamod's existing
        // theme-video error handler falls back to the poster + separate theme audio.
        if (kind === 'video' && mode === 'auto') {
            diagnostics.videoDirectPlays += 1;
            return nativePlay.apply(this, args);
        }

        // Audio requires the Blob bridge on WebKitGTK custom schemes. Video only
        // opts into full buffering in explicit quality mode, where fidelity is
        // preferred over Linux memory/CPU cost.
        return objectUrlFor(this, absolute, kind)
            .then(objectUrl => {
                if (this.dataset) this.dataset.deltamodOriginalMediaSource = absolute;
                if (this.src !== objectUrl) this.src = objectUrl;
                return nativePlay.apply(this, args);
            })
            .catch(error => {
                root.console?.warn?.(`Unable to prepare Linux WebKit ${kind} source ${absolute}:`, error);
                throw error;
            });
    };

    root.addEventListener?.('pagehide', () => {
        for (const objectUrl of [...objectUrls]) revokeObjectUrl(objectUrl);
    }, { once: true });

    return 'active';
});
