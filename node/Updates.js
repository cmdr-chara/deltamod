const { app } = require('electron');
const { autoUpdater } = require('electron-updater');
const console = require('./Console.js');
const { isNewerVersion } = require('./updates/Versioning');

let configured = false;

function configureUpdater() {
    if (configured) return;
    configured = true;
    autoUpdater.autoDownload = false;
    autoUpdater.autoInstallOnAppQuit = true;
    autoUpdater.allowPrerelease = app.getVersion().includes('-');
    autoUpdater.logger = {
        info: (...args) => console.log('[UPDATER]', ...args),
        warn: (...args) => console.warn('[UPDATER]', ...args),
        error: (...args) => console.error('[UPDATER]', ...args),
        debug: (...args) => console.log('[UPDATER/DEBUG]', ...args)
    };
}

async function checkUpdates() {
    configureUpdater();
    if (!app.isPackaged) {
        return {
            update: false,
            version: app.getVersion(),
            releaseName: null,
            reason: 'development-build'
        };
    }

    try {
        const result = await autoUpdater.checkForUpdates();
        const version = result?.updateInfo?.version || app.getVersion();
        return {
            update: isNewerVersion(version, app.getVersion(), {
                allowPrerelease: autoUpdater.allowPrerelease
            }),
            version,
            releaseName: result?.updateInfo?.releaseName || null
        };
    } catch (error) {
        console.warn('Failed to check Community GitHub releases:', error.message);
        return {
            update: false,
            version: app.getVersion(),
            releaseName: null,
            reason: 'check-failed'
        };
    }
}

async function downloadUpdate(onProgress) {
    configureUpdater();
    const progressListener = progress => onProgress?.({
        operationId: 'community-update',
        phase: 'download',
        completed: progress.transferred,
        total: progress.total,
        currentItem: progress.bytesPerSecond,
        percentage: progress.percent
    });
    autoUpdater.on('download-progress', progressListener);
    try {
        return await autoUpdater.downloadUpdate();
    } finally {
        autoUpdater.removeListener('download-progress', progressListener);
    }
}

function installUpdate() {
    configureUpdater();
    autoUpdater.quitAndInstall(false, true);
}

module.exports = {
    checkUpdates,
    downloadUpdate,
    installUpdate
};
