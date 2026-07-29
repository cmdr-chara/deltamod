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

function invoke(channel, data = []) {
    if (!ALLOWED_INVOKE_CHANNELS.has(channel)) {
        return Promise.reject(new Error(`Blocked unknown IPC channel: ${channel}`));
    }
    return ipcRenderer.invoke(channel, data);
}

function on(channel, callback) {
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
}

contextBridge.exposeInMainWorld('communityAPI', {
    app: {
        version: () => invoke('version'),
        platform: () => invoke('getOS'),
        minimize: () => invoke('minimizeMe'),
        toggleFullscreen: () => invoke('toggleFullscreen'),
        openMaintainerProfile: () => invoke('openCommunityMaintainerProfile')
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
    }
});

// Compatibility bridge for existing pages. Unlike the previous bridge, this
// cannot invoke arbitrary main-process channels.
contextBridge.exposeInMainWorld('electronAPI', { invoke });

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
