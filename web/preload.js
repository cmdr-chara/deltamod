const { contextBridge, ipcRenderer } = require('electron');

const ALLOWED_INVOKE_CHANNELS = new Set([
    'htmlAlert_outwin',
    'isCMode',
    'shouldGoIM',
    'diagnosticInfo',
    'isPackaged',
    'version',
    'openCommunityMaintainerProfile',
    'getOS',
    'isDevMode',
    'getOfficialProfileSummary',
    'importOfficialProfile',
    'cancelOfficialProfileImport',
    'restartCommunity',
    'shakeCommunityWindowForEasterEgg',
    'quitCommunityForEasterEgg',
    'sampleError',
    'log',
    'showWindow',
    'minimizeMe',
    'toggleFullscreen',
    'cmode-on',
    'cmode-off',
    'rebootDev',
    'chooseTheme',
    'setTheme',
    'getThemes',
    'getTheme',
    'importTheme',
    'renameCustomTheme',
    'deleteCustomTheme',
    'setSponsor',
    'getSponsor',
    'loginGamebanana',
    'logoutGamebanana',
    'eraseGamebananaCache',
    'leaveCommentGamebanana',
    'gbLikeMod',
    'validateGamebananaToken',
    'getGamebananaPic',
    'getGamebananaID',
    'getGamebananaUserinfo',
    'modSources:getProviders',
    'modSources:browse',
    'modSources:nexusStatus',
    'modSources:startNexusSso',
    'modSources:cancelNexusSso',
    'modSources:clearNexusKey',
    'modSources:open',
    'modSources:downloadNexus',
    'importMod',
    'removeMod',
    'toggleModState',
    'getModState',
    'getModList',
    'getModListFull',
    'howManyMods',
    'dlmodURL',
    'setModVariant',
    'getModImage',
    'precalcGameHashes',
    'getCurrentGameInfo',
    'getGameInfo',
    'getAvailableGames',
    'loadedDeltarune',
    'startGame',
    'gamebanana_getCollections',
    'gamebanana_createCollection',
    'gamebanana_deleteCollection',
    'gamebanana_importToCollection',
    'gamebanana_downloadAllInCollection',
    'patchAndRun',
    'downloadGame',
    'getSystemIndex',
    'getMaxExistingIndex',
    'getInstallations',
    'setInstallationCName',
    'changeSystemIndex',
    'getEditionByIndex',
    'createNewInstallation',
    'cancelGameImport',
    'repairInstallation',
    'reimportInstallation',
    'isCurrentIndexSteam',
    'removeSteamIntegration',
    'deleteSystemIndex',
    'createInstallLink',
    'openInstallationFolder',
    'undertaleModTool:status',
    'undertaleModTool:choose',
    'undertaleModTool:openInstallation',
    'openSysFolder',
    'openModFolder',
    'getUniqueFlag',
    'setUniqueFlag',
    'fetchSharedVariable',
    'isBaked',
    'npsCallback',
    'executeArgumentCmd',
    'openFlagDatabase',
    'deltamoddersDiscord',
    'browseFile',
    'locateDelta',
    'canReportError',
    'fireUpdate',
    'start-update',
    'ignore-update',
    'initialize',
    'modalTest',
    'openElectronTracer',
    'installDeltamodCLI'
]);

const ALLOWED_EVENT_CHANNELS = new Set([
    'page',
    'audio',
    'gplog',
    'updateAvailable',
    'du-progress',
    'themeChange',
    'updateProgress',
    'refresh',
    'finishedPatch',
    'dlmodURL-progress',
    'protocol-download-progress',
    'profile-import-progress',
    'game-import-progress',
    'hash-progress',
    'winResAlert',
    'leave-controller-mode',
    'mod-source-progress'
]);

const ASSET_SCHEMES = Object.freeze({
    app: 'deltapack',
    theme: 'themeprot',
    packet: 'packet'
});

function invoke(channel, data = []) {
    if (!ALLOWED_INVOKE_CHANNELS.has(channel)) {
        return Promise.reject(new Error(`Blocked unknown IPC channel: ${channel}`));
    }
    return ipcRenderer.invoke(channel, data);
}

// Electron implements its full allowlist. Keep errors intact rather than
// treating an Electron failure as an optional Tauri capability.
function invokeOptional(channel, data = [], fallback = undefined) {
    return invoke(channel, data);
}

function isCommandAvailable(channel) {
    return ALLOWED_INVOKE_CHANNELS.has(channel);
}

function on(channel, callback) {
    if (!ALLOWED_EVENT_CHANNELS.has(channel)) {
        throw new Error(`Blocked unknown IPC event channel: ${channel}`);
    }
    if (typeof callback !== 'function') throw new TypeError('IPC event callback must be a function.');
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
}

function assetUrl(kind, assetPath) {
    const scheme = ASSET_SCHEMES[kind];
    if (!scheme) throw new TypeError(`Unknown asset kind: ${kind}`);
    if (typeof assetPath !== 'string' || assetPath.length === 0) {
        throw new TypeError('Asset path must be a non-empty string.');
    }

    let decoded = assetPath;
    for (let pass = 0; pass < 5; pass += 1) {
        let next;
        try {
            next = decodeURIComponent(decoded);
        } catch {
            throw new TypeError('Asset path contains malformed encoding.');
        }
        if (next === decoded) break;
        decoded = next;
    }

    if (
        decoded.includes('\0')
        || decoded.includes('\\')
        || decoded.startsWith('/')
        || /^[A-Za-z]:/.test(decoded)
        || decoded.includes(':')
    ) {
        throw new TypeError('Asset path must be a relative protocol path.');
    }

    const segments = decoded.split('/');
    if (segments.some(segment => segment === '' || segment === '.' || segment === '..')) {
        throw new TypeError('Asset path contains an invalid segment.');
    }

    return `${scheme}://${segments.map(encodeURIComponent).join('/')}`;
}

contextBridge.exposeInMainWorld('communityAPI', {
    app: {
        version: () => invoke('version'),
        platform: () => invoke('getOS'),
        minimize: () => invoke('minimizeMe'),
        toggleFullscreen: () => invoke('toggleFullscreen'),
        openMaintainerProfile: () => invoke('openCommunityMaintainerProfile'),
        shakeForEasterEgg: phase => invoke('shakeCommunityWindowForEasterEgg', [phase]),
        quitForEasterEgg: () => invoke('quitCommunityForEasterEgg')
    },
    profile: {
        summary: () => invoke('getOfficialProfileSummary'),
        import: operationId => invoke('importOfficialProfile', [operationId]),
        cancel: operationId => invoke('cancelOfficialProfileImport', [operationId]),
        onProgress: callback => on('profile-import-progress', callback)
    },
    updates: {
        check: () => invoke('fireUpdate'),
        install: () => invoke('start-update'),
        ignore: () => invoke('ignore-update')
    },
    tools: {
        undertaleModToolStatus: () => invoke('undertaleModTool:status'),
        chooseUndertaleModTool: () => invoke('undertaleModTool:choose'),
        openInstallationInUndertaleModTool: installationIndex =>
            invoke('undertaleModTool:openInstallation', [installationIndex])
    },
    modSources: {
        providers: () => invoke('modSources:getProviders'),
        browse: request => invoke('modSources:browse', [request]),
        nexusStatus: () => invoke('modSources:nexusStatus'),
        startNexusSso: async () => {
            const response = await invoke('modSources:startNexusSso');
            if (!response?.ok) {
                const error = new Error(response?.error?.message || 'Nexus Mods sign-in failed.');
                error.code = response?.error?.code || 'NEXUS_SSO_FAILED';
                throw error;
            }
            return response.status;
        },
        cancelNexusSso: () => invoke('modSources:cancelNexusSso'),
        clearNexusKey: () => invoke('modSources:clearNexusKey'),
        open: request => invoke('modSources:open', [request]),
        downloadNexus: request => invoke('modSources:downloadNexus', [request]),
        onProgress: callback => on('mod-source-progress', callback)
    }
});

// Compatibility bridge for existing pages. Unlike the previous bridge, this
// cannot invoke arbitrary main-process channels.
contextBridge.exposeInMainWorld('electronAPI', { invoke });

contextBridge.exposeInMainWorld('deltamodBackend', {
    invoke,
    invokeOptional,
    isCommandAvailable,
    on,
    assetUrl
});

contextBridge.exposeInMainWorld('preloadAPI', {
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

ipcRenderer.on('warn', (_event, message) => console.warn(message));

Object.defineProperty(navigator, 'mediaSession', {
    value: {
        metadata: null,
        playbackState: 'none',
        setActionHandler: () => {}
    },
    writable: false
});
