(function initializeLinuxMenuAudio(root, factory) {
    if (typeof module === 'object' && module.exports) {
        module.exports = factory;
        return;
    }
    factory(root);
})(typeof window === 'undefined' ? globalThis : window, function installLinuxMenuAudio(root) {
    'use strict';

    const compat = root.DeltamodLinuxCompat;
    const originalAudio = root.audio;
    const AudioConstructor = root.Audio;
    const menuAudio = originalAudio && typeof AudioConstructor === 'function'
        ? new AudioConstructor()
        : originalAudio;
    const fetchAsset = root.fetch?.bind(root);
    const Url = root.URL;
    if (!compat?.isLinuxTauri || !menuAudio || !fetchAsset || !Url?.createObjectURL) {
        return 'inactive';
    }
    if (menuAudio.__deltamodLinuxMenuAudioInstalled) return 'active';

    if (menuAudio !== originalAudio) {
        menuAudio.loop = originalAudio.loop;
        menuAudio.volume = originalAudio.volume;
        root.audio = menuAudio;
        if (typeof root.loopMenuAudio === 'function') {
            menuAudio.addEventListener?.('timeupdate', root.loopMenuAudio);
        }
    }

    const supportedSchemes = new Set(['tauri:', 'themeprot:', 'packet:']);
    const diagnostics = {
        blobLoads: 0,
        coalescedPlays: 0,
        sameSourceSwitches: 0,
        supersededRequests: 0,
        revokedObjectUrls: 0,
        lastError: null,
        webAudioFallbacks: 0,
        webAudioDecodeLoads: 0
    };

    let generation = 0;
    let request = null;
    let webAudioContext = null;
    let webAudioPlayback = null;
    let webAudioSyncTimer = null;
    const webAudioBuffers = new Map();
    // media-compat.js patches HTMLMediaElement.prototype.play. Calling the
    // patched method here would start a second Blob bridge after this
    // coordinator has already assigned its own Blob URL.
    const bridgePlay = typeof compat.playNativeWhenReady === 'function'
        ? (...args) => compat.playNativeWhenReady(menuAudio, ...args)
        : typeof compat.playNative === 'function'
        ? (...args) => compat.playNative(menuAudio, ...args)
        : menuAudio.play.bind(menuAudio);
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

    function stopWebAudioPlayback() {
        if (webAudioSyncTimer !== null) {
            root.clearInterval?.(webAudioSyncTimer);
            webAudioSyncTimer = null;
        }
        const playback = webAudioPlayback;
        webAudioPlayback = null;
        if (!playback) return;
        try {
            playback.source.stop();
        } catch {
            // The source may already have ended or been stopped.
        }
        playback.source.disconnect?.();
        playback.gain.disconnect?.();
    }

    function mediaPlaybackError(error) {
        return error?.name === 'NotSupportedError'
            || error?.name === 'MediaError'
            || error?.code === 4;
    }

    function webAudioContextForPlayback() {
        if (webAudioContext) return Promise.resolve(webAudioContext);
        const Context = root.AudioContext || root.webkitAudioContext;
        if (typeof Context !== 'function') {
            return Promise.reject(new Error('Web Audio is unavailable on this Linux runtime.'));
        }
        try {
            webAudioContext = new Context();
        } catch (error) {
            return Promise.reject(error);
        }
        return Promise.resolve(webAudioContext.resume?.())
            .then(() => webAudioContext);
    }

    function decodedBufferFor(source, blob, context) {
        const cached = webAudioBuffers.get(source);
        if (cached) return Promise.resolve(cached);
        if (typeof blob?.arrayBuffer !== 'function') {
            return Promise.reject(new Error('The menu audio response cannot be decoded.'));
        }
        return blob.arrayBuffer()
            .then(data => context.decodeAudioData(data))
            .then(buffer => {
                webAudioBuffers.set(source, buffer);
                diagnostics.webAudioDecodeLoads += 1;
                return buffer;
            });
    }

    function syncWebAudioPlayback() {
        if (!webAudioPlayback) return;
        const volume = Number(menuAudio.volume);
        webAudioPlayback.gain.gain.value = Number.isFinite(volume)
            ? Math.max(0, Math.min(1, volume))
            : 0;
        webAudioPlayback.source.loop = Boolean(menuAudio.loop);
    }

    function playWebAudio(activeRequest, blob) {
        return webAudioContextForPlayback()
            .then(context => decodedBufferFor(activeRequest.source, blob || activeRequest.blob, context))
            .then(buffer => {
                if (request !== activeRequest || activeRequest.generation !== generation) {
                    throw abortError();
                }

                stopWebAudioPlayback();
                const gain = webAudioContext.createGain();
                const source = webAudioContext.createBufferSource();
                source.buffer = buffer;
                source.loop = Boolean(menuAudio.loop);
                source.connect(gain).connect(webAudioContext.destination);
                source.start();
                webAudioPlayback = { source, gain, sourceName: activeRequest.source };
                diagnostics.webAudioFallbacks += 1;
                syncWebAudioPlayback();

                const schedule = root.setInterval || globalThis.setInterval;
                webAudioSyncTimer = schedule(syncWebAudioPlayback, 250);
                source.onended = () => {
                    if (webAudioPlayback?.source === source && !source.loop) {
                        stopWebAudioPlayback();
                    }
                };
                return 'web-audio-played';
            });
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
            pending: null,
            blob: null
        };

        const pending = fetchAsset(source, controller ? { signal: controller.signal } : undefined)
            .then(response => {
                if (!response?.ok) {
                    throw new Error(`Menu audio request failed with HTTP ${response?.status ?? 'unknown'}.`);
                }
                return response.blob();
            })
            .then(blob => {
                activeRequest.blob = blob;
                const objectUrl = Url.createObjectURL(blob);
                diagnostics.blobLoads += 1;

                if (request !== activeRequest || activeRequest.generation !== generation) {
                    revokeObjectUrl(objectUrl);
                    throw abortError();
                }

                activeRequest.objectUrl = objectUrl;
                storeOriginalSource(source);
                if ('preload' in menuAudio) menuAudio.preload = 'auto';
                if (menuAudio.src !== objectUrl) menuAudio.src = objectUrl;
                return Promise.resolve(bridgePlay(...args)).catch(error => {
                    if (!mediaPlaybackError(error)) throw error;
                    return playWebAudio(activeRequest, blob).catch(() => {
                        throw error;
                    });
                });
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
            if (webAudioPlayback?.sourceName === source) {
                syncWebAudioPlayback();
                diagnostics.coalescedPlays += 1;
                return Promise.resolve();
            }
            if (webAudioBuffers.has(source)) {
                const replay = playWebAudio(request).catch(error => {
                    diagnostics.lastError = error?.message || String(error);
                    throw error;
                });
                const trackedReplay = replay.finally(() => {
                    if (request?.pending === trackedReplay) request.pending = null;
                });
                request.pending = trackedReplay;
                return trackedReplay;
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

    menuAudio.addEventListener?.('pause', stopWebAudioPlayback);
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
                stopWebAudioPlayback();
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
            stopWebAudioPlayback();
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
        stopWebAudioPlayback();
        webAudioBuffers.clear();
        Promise.resolve(webAudioContext?.close?.()).catch(() => {});
        webAudioContext = null;
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
