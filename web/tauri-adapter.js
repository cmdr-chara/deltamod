(function initializeTauriAdapter(root, factory) {
    if (typeof module === 'object' && module.exports) {
        module.exports = factory;
        return;
    }
    factory(root);
})(typeof window === 'undefined' ? globalThis : window, function installTauriAdapter(root) {
    'use strict';

    if (root.deltamodBackend || root.communityAPI || root.preloadAPI) return 'electron';
    if (!root.__TAURI__?.core?.invoke || !root.__TAURI__?.event?.listen) return 'browser';

    const tauriInvoke = root.__TAURI__.core.invoke;
    const listen = root.__TAURI__.event.listen;
    const unsupportedCommands = new Set([
        'htmlAlert_outwin', 'shouldGoIM', 'sampleError',
        'rebootDev', 'setSponsor',
        'shakeCommunityWindowForEasterEgg', 'createInstallLink',
        'undertaleModTool:openInstallation', 'gamebanana_downloadAllInCollection', 'start-update',
        'ignore-update', 'getEditionByIndex', 'openFlagDatabase',
        'deltamoddersDiscord', 'canReportError', 'npsCallback', 'executeArgumentCmd', 'initialize',
        'modalTest', 'openElectronTracer', 'installDeltamodCLI'
    ]);
    const allowedEvents = new Set([
        'page', 'audio', 'gplog', 'updateAvailable', 'du-progress', 'themeChange',
        'updateProgress', 'refresh', 'finishedPatch', 'dlmodURL-progress',
        'protocol-download-progress', 'profile-import-progress', 'game-import-progress',
        'hash-progress', 'winResAlert', 'leave-controller-mode', 'mod-source-progress'
    ]);

    function invoke(channel, data = []) {
        if (typeof channel !== 'string' || channel.length === 0) {
            return Promise.reject(new TypeError('Backend channel must be a non-empty string.'));
        }
        if (!Array.isArray(data)) {
            return Promise.reject(new TypeError('Backend payload must be an array.'));
        }
        return tauriInvoke('backend_invoke', { channel, data });
    }

    async function invokeOptional(channel, data = [], fallback = undefined) {
        try {
            return await invoke(channel, data);
        } catch (error) {
            if (String(error?.message || error) === `TAURI_COMMAND_UNAVAILABLE:${channel}`) {
                return fallback;
            }
            throw error;
        }
    }

    function isCommandAvailable(channel) {
        return !unsupportedCommands.has(channel);
    }

    function on(channel, callback) {
        if (!allowedEvents.has(channel)) {
            throw new Error(`Blocked unknown IPC event channel: ${channel}`);
        }
        if (typeof callback !== 'function') throw new TypeError('IPC event callback must be a function.');

        let disposed = false;
        let unlisten = null;
        Promise.resolve(listen(channel, event => callback(event.payload))).then(handle => {
            if (disposed) handle();
            else unlisten = handle;
        }).catch(error => {
            if (!disposed) root.console?.error?.(`Unable to subscribe to ${channel}:`, error);
        });

        return () => {
            if (disposed) return;
            disposed = true;
            if (unlisten) unlisten();
        };
    }

    function validateAssetPath(assetPath) {
        if (typeof assetPath !== 'string' || assetPath.length === 0) {
            throw new TypeError('Asset path must be a non-empty string.');
        }
        let decoded = assetPath;
        for (let pass = 0; pass < 5; pass += 1) {
            let next;
            try { next = decodeURIComponent(decoded); }
            catch { throw new TypeError('Asset path contains malformed encoding.'); }
            if (next === decoded) break;
            decoded = next;
        }
        if (decoded.includes('\0') || decoded.includes('\\') || decoded.startsWith('/')
            || /^[A-Za-z]:/.test(decoded) || decoded.includes(':')) {
            throw new TypeError('Asset path must be a relative app path.');
        }
        const segments = decoded.split('/');
        if (segments.some(segment => !segment || segment === '.' || segment === '..')) {
            throw new TypeError('Asset path contains an invalid segment.');
        }
        return segments.map(encodeURIComponent).join('/');
    }

    function assetUrl(kind, assetPath) {
        if (kind !== 'app' && kind !== 'theme' && kind !== 'packet') {
            throw new TypeError(`Unknown asset kind: ${kind}`);
        }
        const path = validateAssetPath(assetPath);
        if (kind === 'theme') return `themeprot://asset/${path}`;
        if (kind === 'packet') return `packet://${path}`;
        const relative = path.startsWith('web/') ? path.slice(4) : path;
        return new URL(relative, root.location.href).href;
    }

    const backend = Object.freeze({ invoke, invokeOptional, isCommandAvailable, on, assetUrl });
    root.deltamodBackend = backend;
    root.electronAPI = Object.freeze({ invoke });
    root.communityAPI = Object.freeze({
        app: Object.freeze({
            version: () => invoke('version'),
            platform: () => invoke('getOS'),
            minimize: () => invoke('minimizeMe'),
            toggleFullscreen: () => invoke('toggleFullscreen'),
            openMaintainerProfile: () => invoke('openCommunityMaintainerProfile'),
            shakeForEasterEgg: phase => invoke('shakeCommunityWindowForEasterEgg', [phase]),
            quitForEasterEgg: () => invoke('quitCommunityForEasterEgg')
        }),
        profile: Object.freeze({
            summary: () => invoke('getOfficialProfileSummary'),
            import: operationId => invoke('importOfficialProfile', [operationId]),
            cancel: operationId => invoke('cancelOfficialProfileImport', [operationId]),
            onProgress: callback => on('profile-import-progress', callback)
        }),
        updates: Object.freeze({
            check: () => invoke('fireUpdate'),
            install: () => invoke('start-update'),
            ignore: () => invoke('ignore-update')
        }),
        tools: Object.freeze({
            undertaleModToolStatus: () => invoke('undertaleModTool:status'),
            chooseUndertaleModTool: () => invoke('undertaleModTool:choose'),
            openInstallationInUndertaleModTool: index => invoke('undertaleModTool:openInstallation', [index])
        }),
        modSources: Object.freeze({
            providers: () => invoke('modSources:getProviders'),
            browse: request => invoke('modSources:browse', [request]),
            nexusStatus: () => invoke('modSources:nexusStatus'),
            startNexusSso: () => invoke('modSources:startNexusSso').then(response => {
                if (response?.ok) return response.status;
                const error = new Error(response?.error?.message || 'Nexus Mods sign-in failed.');
                error.code = response?.error?.code || 'NEXUS_SSO_FAILED';
                throw error;
            }),
            cancelNexusSso: () => invokeOptional('modSources:cancelNexusSso', [], false),
            clearNexusKey: () => invoke('modSources:clearNexusKey'),
            open: request => invoke('modSources:open', [request]),
            downloadNexus: request => invoke('modSources:downloadNexus', [request]),
            onProgress: callback => on('mod-source-progress', callback)
        })
    });
    root.preloadAPI = Object.freeze({
        onPage: callback => on('page', callback),
        onAudio: callback => on('audio', callback),
        onGPL: callback => on('gplog', callback),
        onUpdateAvailable: callback => on('updateAvailable', callback),
        onDDS: callback => on('du-progress', callback),
        onThemeChange: callback => on('themeChange', callback),
        onUpdateProgress: callback => on('updateProgress', callback),
        onRefresh: callback => on('refresh', callback),
        onFinishedPatch: callback => on('finishedPatch', callback),
        onDLMODProgress: callback => on('dlmodURL-progress', callback),
        onProtocolDownloadProgress: callback => on('protocol-download-progress', callback),
        onProfileImportProgress: callback => on('profile-import-progress', callback),
        onGameImportProgress: callback => on('game-import-progress', callback),
        onHashProgress: callback => on('hash-progress', callback),
        onWRA: callback => on('winResAlert', callback),
        onLeaveControllerMode: callback => on('leave-controller-mode', callback)
    });
    return 'tauri';
});
