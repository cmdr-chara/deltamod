(function initializeLinuxMenuAudio(root, factory) {
    if (typeof module === 'object' && module.exports) {
        module.exports = factory;
        return;
    }
    factory(root);
})(typeof window === 'undefined' ? globalThis : window, function installLinuxMenuAudio(root) {
    'use strict';

    const compat = root.DeltamodLinuxCompat;
    const menuAudio = root.audio;
    const fetchAsset = root.fetch?.bind(root);
    const Url = root.URL;
    if (!compat?.isLinuxTauri || !menuAudio || !fetchAsset || !Url?.createObjectURL) {
        return 'inactive';
    }
    if (menuAudio.__deltamodLinuxMenuAudioInstalled) return 'active';

    const supportedSchemes = new Set(['tauri:', 'themeprot:', 'packet:']);
    const diagnostics = {
        blobLoads: 0,
        coalescedPlays: 0,
        sameSourceSwitches: 0,
        supersededRequests: 0,
        revokedObjectUrls: 0,
        lastError: null
    };

    let generation = 0;
    let request = null;
    const bridgePlay = menuAudio.play.bind(menuAudio);
    const nativeSwitch = typeof root.switchMenuAudioSource === 'function'
        ? root.switchMenuAudioSource
        : null;
    const nativeRelease = typeof root.releaseAudioBuffer === 'function'
        ? root.releaseAudioBuffer
        : null;

    function normalizeSource(rawSource) {
        if (!rawSource) return null;
        try {
            return new Url(rawSource, root.location?.href).href;
        } catch {
            return null;
        }
    }

    function isCompatibilitySource(source) {
        if (!source) return false;
        try {
            return supportedSchemes.has(new Url(source).protocol);
        } catch {
            return false;
        }
    }

    function abortError(message = 'Menu audio request was superseded.') {
        if (typeof root.DOMException === 'function') {
            return new root.DOMException(message, 'AbortError');
        }
        const error = new Error(message);
        error.name = 'AbortError';
        return error;
    }

    function revokeObjectUrl(objectUrl) {
        if (!objectUrl) return;
        Url.revokeObjectURL?.(objectUrl);
        diagnostics.revokedObjectUrls += 1;
    }

    function abortRequest(activeRequest, { revoke = false } = {}) {
        if (!activeRequest) return;
        activeRequest.controller?.abort?.();
        if (revoke && activeRequest.objectUrl) {
            revokeObjectUrl(activeRequest.objectUrl);
            activeRequest.objectUrl = null;
        }
    }

    function clearRequest({ revoke = false } = {}) {
        const activeRequest = request;
        request = null;
        generation += 1;
        abortRequest(activeRequest, { revoke });
    }

    function sourceForMenuAudio() {
        const intended = normalizeSource(root.currentAudioSource);
        if (intended) return intended;

        const original = normalizeSource(menuAudio.dataset?.deltamodOriginalMediaSource);
        if (original) return original;

        return normalizeSource(menuAudio.src || menuAudio.currentSrc);
    }

    function sameActiveSource(source) {
        const normalized = normalizeSource(source);
        if (!normalized) return false;
        if (request?.source === normalized) return true;
        return normalizeSource(root.currentAudioSource) === normalized
            && Boolean(menuAudio.src || menuAudio.currentSrc);
    }

    function storeOriginalSource(source) {
        if (menuAudio.dataset) {
            menuAudio.dataset.deltamodOriginalMediaSource = source;
        } else if (typeof menuAudio.setAttribute === 'function') {
            menuAudio.setAttribute('data-deltamod-original-media-source', source);
        }
    }

    function prepareAndPlay(source, args) {
        const requestGeneration = ++generation;
        const Controller = root.AbortController;
        const controller = typeof Controller === 'function' ? new Controller() : null;
        const activeRequest = {
            generation: requestGeneration,
            source,
            controller,
            objectUrl: null,
            pending: null
        };

        const pending = fetchAsset(source, controller ? { signal: controller.signal } : undefined)
            .then(response => {
                if (!response?.ok) {
                    throw new Error(`Menu audio request failed with HTTP ${response?.status ?? 'unknown'}.`);
                }
                return response.blob();
            })
            .then(blob => {
                const objectUrl = Url.createObjectURL(blob);
                diagnostics.blobLoads += 1;

                if (request !== activeRequest || activeRequest.generation !== generation) {
                    revokeObjectUrl(objectUrl);
                    throw abortError();
                }

                activeRequest.objectUrl = objectUrl;
                storeOriginalSource(source);
                if (menuAudio.src !== objectUrl) menuAudio.src = objectUrl;
                return bridgePlay(...args);
            })
            .catch(error => {
                if (error?.name !== 'AbortError') {
                    diagnostics.lastError = error?.message || String(error);
                    root.console?.warn?.('Unable to prepare Linux menu audio.', error);
                }
                throw error;
            })
            .finally(() => {
                if (request === activeRequest) activeRequest.pending = null;
            });

        activeRequest.pending = pending;
        request = activeRequest;
        return pending;
    }

    function coordinatedPlay(...args) {
        const source = sourceForMenuAudio();
        if (!isCompatibilitySource(source)) {
            return bridgePlay(...args);
        }

        if (request?.source === source) {
            if (request.pending) {
                diagnostics.coalescedPlays += 1;
                return request.pending;
            }
            if (request.objectUrl) {
                if (menuAudio.src !== request.objectUrl) menuAudio.src = request.objectUrl;
                if (menuAudio.paused === false) {
                    diagnostics.coalescedPlays += 1;
                    return Promise.resolve();
                }
                const replay = Promise.resolve(bridgePlay(...args));
                const trackedReplay = replay.finally(() => {
                    if (request?.pending === trackedReplay) request.pending = null;
                });
                request.pending = trackedReplay;
                return trackedReplay;
            }
        }

        if (request && request.source !== source) {
            diagnostics.supersededRequests += 1;
            clearRequest({ revoke: true });
        }
        return prepareAndPlay(source, args);
    }

    menuAudio.play = coordinatedPlay;
    Object.defineProperty(menuAudio, '__deltamodLinuxMenuAudioInstalled', {
        value: true,
        configurable: true
    });

    if (nativeSwitch && !nativeSwitch.__deltamodLinuxMenuAudioCoordinated) {
        const coordinatedSwitch = function coordinatedMenuAudioSwitch(source, ...args) {
            const normalized = normalizeSource(source);
            if (normalized && sameActiveSource(normalized)) {
                diagnostics.sameSourceSwitches += 1;
                root.configureMenuAudioPlayback?.(source);
                return false;
            }

            const previousRequest = request;
            if (previousRequest && previousRequest.source !== normalized) {
                diagnostics.supersededRequests += 1;
                request = null;
                generation += 1;
                abortRequest(previousRequest, { revoke: false });
            }

            try {
                return nativeSwitch.call(this, source, ...args);
            } finally {
                if (previousRequest?.objectUrl) {
                    revokeObjectUrl(previousRequest.objectUrl);
                    previousRequest.objectUrl = null;
                }
            }
        };
        coordinatedSwitch.__deltamodLinuxMenuAudioCoordinated = true;
        root.switchMenuAudioSource = coordinatedSwitch;
    }

    if (nativeRelease && !nativeRelease.__deltamodLinuxMenuAudioCoordinated) {
        const coordinatedRelease = function coordinatedMenuAudioRelease(...args) {
            const previousRequest = request;
            request = null;
            generation += 1;
            abortRequest(previousRequest, { revoke: false });
            try {
                return nativeRelease.apply(this, args);
            } finally {
                if (previousRequest?.objectUrl) {
                    revokeObjectUrl(previousRequest.objectUrl);
                    previousRequest.objectUrl = null;
                }
                if (menuAudio.dataset) delete menuAudio.dataset.deltamodOriginalMediaSource;
            }
        };
        coordinatedRelease.__deltamodLinuxMenuAudioCoordinated = true;
        root.releaseAudioBuffer = coordinatedRelease;
    }

    root.addEventListener?.('pagehide', () => {
        clearRequest({ revoke: true });
    }, { once: true });

    root.DeltamodLinuxMenuAudio = Object.freeze({
        snapshot: () => Object.freeze({
            ...diagnostics,
            source: request?.source || null,
            pending: Boolean(request?.pending),
            objectUrlActive: Boolean(request?.objectUrl),
            paused: menuAudio.paused,
            readyState: menuAudio.readyState,
            currentTime: menuAudio.currentTime
        })
    });

    return 'active';
});
