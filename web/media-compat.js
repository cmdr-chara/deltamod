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

    root.document?.documentElement?.classList?.add('deltamod-linux-webkit');

    const MediaElement = root.HTMLMediaElement;
    const fetchAsset = root.fetch?.bind(root);
    const Url = root.URL;
    if (!MediaElement?.prototype?.play || !fetchAsset || !Url?.createObjectURL) {
        return 'styles-only';
    }

    const supportedSchemes = new Set(['tauri:', 'themeprot:', 'packet:']);
    const nativePlay = MediaElement.prototype.play;
    const objectUrls = new Set();
    const objectUrlCache = new Map();

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

    function mediaSourceFor(element) {
        const assignedSource = element.src
            || element.getAttribute?.('src')
            || element.currentSrc;

        if (needsCompatibilitySource(assignedSource)) return assignedSource;

        const absoluteAssignedSource = normalizeSource(assignedSource);
        const originalSource = element.dataset?.deltamodOriginalMediaSource;
        if (absoluteAssignedSource?.startsWith('blob:') && originalSource) {
            return originalSource;
        }

        if (originalSource && element.dataset && absoluteAssignedSource) {
            delete element.dataset.deltamodOriginalMediaSource;
        }
        return assignedSource;
    }

    function objectUrlFor(source) {
        const absolute = normalizeSource(source);
        if (!absolute) return Promise.reject(new TypeError('Invalid media source.'));

        let pending = objectUrlCache.get(absolute);
        if (pending) return pending;

        pending = fetchAsset(absolute)
            .then(response => {
                if (!response.ok) {
                    throw new Error(`Media request failed with HTTP ${response.status}.`);
                }
                return response.blob();
            })
            .then(blob => {
                const objectUrl = Url.createObjectURL(blob);
                objectUrls.add(objectUrl);
                return objectUrl;
            })
            .catch(error => {
                objectUrlCache.delete(absolute);
                throw error;
            });

        objectUrlCache.set(absolute, pending);
        return pending;
    }

    MediaElement.prototype.play = function patchedPlay(...args) {
        const source = mediaSourceFor(this);
        if (!needsCompatibilitySource(source)) {
            return nativePlay.apply(this, args);
        }

        const absolute = normalizeSource(source);
        return objectUrlFor(absolute)
            .then(objectUrl => {
                if (this.dataset) this.dataset.deltamodOriginalMediaSource = absolute;
                if (this.src !== objectUrl) this.src = objectUrl;
                return nativePlay.apply(this, args);
            })
            .catch(error => {
                root.console?.warn?.(`Unable to prepare Linux WebKit media source ${absolute}:`, error);
                throw error;
            });
    };

    root.addEventListener?.('pagehide', () => {
        for (const objectUrl of objectUrls) Url.revokeObjectURL?.(objectUrl);
        objectUrls.clear();
        objectUrlCache.clear();
    }, { once: true });

    return 'active';
});
