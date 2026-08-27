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
        sharedAudioBlobLoads: 0,
        sharedAudioCacheHits: 0,
        videoBlobLoads: 0,
        videoDirectPlays: 0,
        videoBlocks: 0,
        videoCodecBlocks: 0,
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
    const sharedAudioObjectUrls = new Map();
    let nativePlay = null;

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

    function playNative(element, ...args) {
        if (typeof nativePlay !== 'function') {
            return Promise.reject(new Error('Native media playback is unavailable.'));
        }
        return nativePlay.apply(element, args);
    }

    function waitForMediaReady(element) {
        if (!element?.addEventListener || Number(element.readyState) >= 2) {
            return Promise.resolve();
        }

        return new Promise((resolve, reject) => {
            let settled = false;
            let timeoutId = null;
            const events = ['loadeddata', 'canplay'];

            const cleanup = () => {
                events.forEach(name => element.removeEventListener?.(name, onReady));
                element.removeEventListener?.('error', onError);
                if (timeoutId !== null) root.clearTimeout?.(timeoutId);
            };

            const finish = (callback, value) => {
                if (settled) return;
                settled = true;
                cleanup();
                callback(value);
            };

            const onReady = () => {
                if (Number(element.readyState) >= 2) finish(resolve);
            };
            const onError = () => {
                finish(reject, element.error || new Error('Media element failed while loading.'));
            };

            events.forEach(name => element.addEventListener(name, onReady));
            element.addEventListener('error', onError);
            onReady();

            const schedule = root.setTimeout || globalThis.setTimeout;
            const timeoutMs = String(element.tagName || '').toUpperCase() === 'VIDEO'
                ? 20000
                : 5000;
            timeoutId = schedule(() => {
                if (Number(element.readyState) >= 2) {
                    finish(resolve);
                } else if (element.error) {
                    finish(reject, element.error);
                } else {
                    // Let native play() produce the final decoder error when
                    // a WebKit build never emits a readiness event.
                    finish(resolve);
                }
            }, timeoutMs);
        });
    }

    function playNativeWhenReady(element, ...args) {
        return waitForMediaReady(element).then(() => playNative(element, ...args));
    }

    applyModeClasses();

    const compatApi = Object.freeze({
        isLinuxTauri: true,
        getMode: () => mode,
        setMode,
        usesReducedEffects: () => mode !== 'quality',
        forcesPosterVideo: () => mode !== 'quality',
        playNative,
        playNativeWhenReady,
        snapshot: () => Object.freeze({ mode, ...diagnostics })
    });
    root.DeltamodLinuxCompat = compatApi;

    const MediaElement = root.HTMLMediaElement;
    const fetchAsset = root.fetch?.bind(root);
    const Url = root.URL;
    if (!MediaElement?.prototype?.play || !fetchAsset || !Url?.createObjectURL) {
        return 'styles-only';
    }

    nativePlay = MediaElement.prototype.play;

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
        if (!state.shared) revokeObjectUrl(state.objectUrl);
        elementStates.delete(element);
        if (!keepOriginal && element.dataset) {
            delete element.dataset.deltamodOriginalMediaSource;
        }
    }

    function attachElementCleanup(element) {
        if (cleanupAttached.has(element) || !element?.addEventListener) return;
        cleanupAttached.add(element);
        element.addEventListener('ended', () => {
            const retainBlob = element.dataset?.deltamodRetainMediaBlob === 'true';
            if (!element.loop && !retainBlob) {
                releaseElementObjectUrl(element, { keepOriginal: true });
            }
        });
        element.addEventListener('emptied', () => {
            const state = elementStates.get(element);
            const assignedSource = normalizeSource(
                element.src || element.getAttribute?.('src') || element.currentSrc
            );
            // applyThemeBackground cancels the old custom-scheme video before
            // installing its Blob source. WebKitGTK can deliver that old
            // emptied event after the new bridge state exists; keep the state
            // while the theme element is still associated with that source.
            if (state?.kind === 'video'
                && !assignedSource
                && normalizeSource(element.dataset?.source) === state.source) {
                return;
            }
            releaseElementObjectUrl(element);
        });
    }

    function fetchMediaObjectUrl(absolute, kind) {
        return fetchAsset(absolute)
            .then(response => {
                if (!response.ok) {
                    throw new Error(`Media request failed with HTTP ${response.status}.`);
                }
                return response.blob();
            })
            .then(blob => {
                const objectUrl = Url.createObjectURL(blob);
                objectUrls.add(objectUrl);
                diagnostics[kind === 'video' ? 'videoBlobLoads' : 'audioBlobLoads'] += 1;
                return objectUrl;
            });
    }

    function isReusableAppSfxSource(absolute) {
        try {
            const parsed = new Url(absolute);
            return parsed.protocol === 'tauri:'
                && parsed.hostname === 'localhost'
                && /^\/audio\/[^/]+\.(?:mp3|ogg|wav)$/i.test(parsed.pathname);
        } catch {
            return false;
        }
    }

    function sharedAudioObjectUrlFor(absolute) {
        const cached = sharedAudioObjectUrls.get(absolute);
        if (cached) {
            diagnostics.sharedAudioCacheHits += 1;
            return cached;
        }

        const pending = fetchMediaObjectUrl(absolute, 'audio')
            .then(objectUrl => {
                diagnostics.sharedAudioBlobLoads += 1;
                return objectUrl;
            })
            .catch(error => {
                sharedAudioObjectUrls.delete(absolute);
                throw error;
            });
        sharedAudioObjectUrls.set(absolute, pending);
        return pending;
    }

    function mediaObjectUrlFor(element, source, kind, shared = false) {
        const absolute = normalizeSource(source);
        if (!absolute) return Promise.reject(new TypeError('Invalid media source.'));

        const existing = elementStates.get(element);
        if (existing?.source === absolute) {
            if (existing.objectUrl) return Promise.resolve(existing.objectUrl);
            if (existing.pending) return existing.pending;
        } else if (existing) {
            releaseElementObjectUrl(element);
        }

        const state = { source: absolute, objectUrl: null, pending: null, shared, kind };
        const pending = (shared
            ? sharedAudioObjectUrlFor(absolute)
            : fetchMediaObjectUrl(absolute, kind))
            .then(objectUrl => {
                if (elementStates.get(element) !== state) {
                    if (!shared) revokeObjectUrl(objectUrl);
                    const error = new Error('Media source changed while the Blob bridge was loading.');
                    error.code = 'DELTAMOD_MEDIA_SOURCE_CHANGED';
                    throw error;
                }
                state.objectUrl = objectUrl;
                state.pending = null;
                return objectUrl;
            })
            .catch(error => {
                if (elementStates.get(element) === state) {
                    elementStates.delete(element);
                }
                diagnostics.lastError = `blob-load: ${error?.message || error}`;
                throw error;
            });

        state.pending = pending;
        elementStates.set(element, state);
        attachElementCleanup(element);
        return pending;
    }

    function audioObjectUrlFor(element, source) {
        return mediaObjectUrlFor(element, source, 'audio', isReusableAppSfxSource(normalizeSource(source)));
    }

    function videoObjectUrlFor(element, source) {
        return mediaObjectUrlFor(element, source, 'video');
    }

    function playBufferedMedia(element, source, kind, args) {
        if (kind === 'video' && !elementStates.has(element)) {
            // applyThemeBackground assigns the custom URI before calling
            // play(). Cancel that native WebKit load before installing the
            // Blob bridge; its late `emptied` event otherwise invalidates the
            // newly-created request and produces a false source-change error.
            const assignedSource = normalizeSource(
                element.src || element.getAttribute?.('src') || element.currentSrc
            );
            if (assignedSource === source) {
                element.pause?.();
                element.removeAttribute?.('src');
                element.load?.();
            }
        }
        const objectUrl = kind === 'video'
            ? videoObjectUrlFor(element, source)
            : audioObjectUrlFor(element, source);
        return objectUrl.then(value => {
            if (element.dataset) element.dataset.deltamodOriginalMediaSource = source;
            if ('preload' in element) element.preload = 'auto';
            if (element.src !== value) element.src = value;
            return playNativeWhenReady(element, ...args);
        });
    }

    function isThemeBackgroundVideo(element) {
        return element?.id === 'theme-background-video'
            || element?.classList?.contains?.('theme-background-video');
    }

    function posterVideoError(source) {
        const error = new Error(`Linux ${mode} mode uses the theme poster instead of video: ${source}`);
        error.name = 'NotSupportedError';
        error.code = 'DELTAMOD_LINUX_THEME_VIDEO_DISABLED';
        return error;
    }

    function supportsThemeVideoCodecs(element, source) {
        if (!isThemeBackgroundVideo(element) || typeof element?.canPlayType !== 'function') {
            return true;
        }
        try {
            const parsed = new Url(source);
            if (!/\.mp4$/i.test(parsed.pathname)) return true;
        } catch {
            return true;
        }
        return Boolean(element.canPlayType('video/mp4; codecs="avc1.4D401E, mp4a.40.2"'));
    }

    function codecVideoError(source) {
        const error = new Error(
            `Linux WebKitGTK cannot decode the theme H.264/AAC video (${source}). `
            + 'Install the platform GStreamer codec plugin (Arch: gst-libav) or use Auto/Performance mode.'
        );
        error.name = 'NotSupportedError';
        error.code = 'DELTAMOD_LINUX_VIDEO_CODEC_UNAVAILABLE';
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

        if (kind === 'video') {
            if (isThemeBackgroundVideo(this) && mode !== 'quality') {
                diagnostics.videoBlocks += 1;
                const error = posterVideoError(absolute);
                diagnostics.lastError = error.code;
                root.console?.warn?.(error.message);
                return Promise.reject(error);
            }

            if (!supportsThemeVideoCodecs(this, absolute)) {
                diagnostics.videoCodecBlocks += 1;
                const error = codecVideoError(absolute);
                diagnostics.lastError = error.code;
                root.console?.warn?.(error.message);
                return Promise.reject(error);
            }

            // WebKitGTK rejects MP4 served through Tauri's custom URI scheme on
            // some Linux builds even when the installed GStreamer codecs work.
            // Buffer the single theme video once in Quality mode, while keeping
            // non-theme video on the native stream so large previews retain
            // Range support.
            if (isThemeBackgroundVideo(this)) {
                return playBufferedMedia(this, absolute, 'video', args)
                    .catch(error => {
                        root.console?.warn?.(`Unable to prepare Linux WebKit theme video ${absolute}:`, error);
                        throw error;
                    });
            }

            diagnostics.videoDirectPlays += 1;
            return playNative(this, ...args);
        }

        // Custom-scheme audio still needs a Blob bridge on WebKitGTK. Repeated
        // built-in SFX share one Blob URL to avoid player/request churn.
        return playBufferedMedia(this, absolute, 'audio', args)
            .catch(error => {
                root.console?.warn?.(`Unable to prepare Linux WebKit audio source ${absolute}:`, error);
                throw error;
            });
    };

    root.addEventListener?.('pagehide', () => {
        for (const objectUrl of [...objectUrls]) revokeObjectUrl(objectUrl);
        sharedAudioObjectUrls.clear();
    }, { once: true });

    return 'active';
});
