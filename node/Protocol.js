const fs = require('fs');
const path = require('path');
const mime = require('mime-types');
const { app, dialog } = require('electron');
const { log } = require('./Console');
const { isFeatureEnabled } = require('./FeatureFlags');
const System = require('./System');
const { importMod } = require('./Modstore');
const { page, setSharedVar, getWindow } = require('./Utils');
const { errorWin } = require('./ErrorWin');
const { protocolPath, resolveWithin } = require('./security/PathSecurity');
const { downloadToFile } = require('./security/RemoteSecurity');
const { APPLICATION_SCHEME, parseLaunch } = require('./protocol/LaunchParser');
const { stageLocalArchive } = require('./protocol/LocalModImport');
const { writeFileAtomicSync } = require('./storage/AtomicStore');

const MAXIMUM_MOD_DOWNLOAD_BYTES = 2 * 1024 * 1024 * 1024;
const PACKET_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.webp', '.gif', '.bmp']);
const THEME_EXTENSIONS = new Set(['.json', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.mp3', '.ogg', '.wav']);

function responseError(status, message) {
    return new Response(message, {
        status,
        headers: {
            'Content-Type': 'text/plain; charset=utf-8',
            'Cache-Control': 'no-store'
        }
    });
}

async function fileResponse(filePath, allowedExtensions = null) {
    if (allowedExtensions && !allowedExtensions.has(path.extname(filePath).toLowerCase())) {
        return responseError(403, 'File type is not allowed by this protocol.');
    }
    try {
        const data = await fs.promises.readFile(filePath);
        return new Response(data, {
            headers: {
                'Content-Type': mime.lookup(filePath) || 'application/octet-stream',
                'Content-Length': String(data.length),
                'Cache-Control': 'no-cache',
                'X-Content-Type-Options': 'nosniff'
            }
        });
    } catch (error) {
        return responseError(error.code === 'ENOENT' ? 404 : 500, 'Resource could not be loaded.');
    }
}

function registerProtocolSchemesAsPrivileged(protocol) {
    const localPrivileges = {
        standard: true,
        secure: true,
        supportFetchAPI: true
    };
    const crossOriginAssetPrivileges = {
        ...localPrivileges,
        corsEnabled: true
    };
    protocol.registerSchemesAsPrivileged([
        { scheme: 'deltapack', privileges: localPrivileges },
        { scheme: 'packet', privileges: crossOriginAssetPrivileges },
        { scheme: 'themeprot', privileges: crossOriginAssetPrivileges },
        { scheme: APPLICATION_SCHEME, privileges: { standard: true, secure: true } }
    ]);
}

function registerProtocolHandlers(session) {
    session.protocol.handle('deltapack', async request => {
        try {
            const relative = protocolPath(request.url);
            const applicationRoot = path.resolve(__dirname, '..');
            return fileResponse(resolveWithin(applicationRoot, relative, { mustExist: true }));
        } catch (error) {
            log('Blocked deltapack request:', error.message);
            return responseError(403, 'Blocked application resource path.');
        }
    });

    session.protocol.handle('themeprot', async request => {
        try {
            const relative = protocolPath(request.url);
            const builtInRoot = path.resolve(__dirname, '..', 'web', 'themes');
            const customRoot = path.join(app.getPath('userData'), 'customThemes');
            await fs.promises.mkdir(customRoot, { recursive: true });

            const builtInPath = resolveWithin(builtInRoot, relative);
            const customPath = resolveWithin(customRoot, relative);
            const selected = fs.existsSync(builtInPath) ? builtInPath : customPath;
            return fileResponse(selected, THEME_EXTENSIONS);
        } catch (error) {
            log('Blocked theme request:', error.message);
            return responseError(403, 'Blocked theme resource path.');
        }
    });

    session.protocol.handle('packet', async request => {
        try {
            const relative = protocolPath(request.url);
            const filePath = resolveWithin(System.getPacketDatabase(), relative, { mustExist: true });
            return fileResponse(filePath, PACKET_EXTENSIONS);
        } catch (error) {
            log('Blocked packet request:', error.message);
            return responseError(403, 'Blocked mod resource path.');
        }
    });

    session.protocol.handle('http', async () => responseError(403, 'HTTP is not supported. Use HTTPS.'));
}

async function installGameBananaMod(argumentsList) {
    if (!isFeatureEnabled('GB-OneClick') || argumentsList.length < 3) return;

    const modType = String(argumentsList.shift());
    const modId = String(argumentsList.shift());
    const archiveUrl = argumentsList.join('/');
    if (!/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(modType) || !/^\d+$/.test(modId)) {
        throw new Error('The GameBanana mod type or submission ID is invalid.');
    }

    setSharedVar('gb1click', true);
    const itemId = System.generateUniqueId();
    const filePath = path.join(System.getTemporary(), `${itemId}.modarchive`);
    const window = getWindow();

    try {
        await downloadToFile(archiveUrl, filePath, {
            maximumBytes: MAXIMUM_MOD_DOWNLOAD_BYTES,
            onProgress: progress => {
                const percentage = progress.total > 0
                    ? Math.floor((progress.completed / progress.total) * 100)
                    : null;
                window?.webContents.send('protocol-download-progress', {
                    operationId: itemId,
                    phase: 'download',
                    completed: progress.completed,
                    total: progress.total,
                    currentItem: archiveUrl,
                    percentage
                });
            }
        });

        await importMod(filePath, 'main', modId, modType);
    } finally {
        setSharedVar('gb1click', false);
        await fs.promises.rm(filePath, { force: true });
    }
}

async function installLocalModArchive(sourcePath) {
    if (typeof sourcePath !== 'string' || !sourcePath.trim()) {
        throw new Error('The local mod archive path is missing.');
    }
    const dialogOptions = {
        type: 'question',
        title: 'Import local mod',
        message: 'Import this mod package into Deltamod Community?',
        detail: sourcePath,
        buttons: ['Import', 'Cancel'],
        defaultId: 1,
        cancelId: 1,
        noLink: true
    };
    const owner = getWindow();
    const confirmation = owner
        ? await dialog.showMessageBox(owner, dialogOptions)
        : await dialog.showMessageBox(dialogOptions);
    if (confirmation.response !== 0) return false;

    const stagedPath = await stageLocalArchive(sourcePath, System.getTemporary());
    try {
        const imported = await importMod(stagedPath, 'main');
        if (imported !== true) throw new Error('The local mod package was not imported.');
        return true;
    } finally {
        await fs.promises.rm(stagedPath, { force: true });
    }
}

async function handleProtocolLaunch(value) {
    try {
        const launch = parseLaunch(value);
        if (!launch) return;
        log('Community protocol launch detected:', launch.command);
        switch (launch.command) {
            case 'gb':
                await installGameBananaMod(launch.arguments);
                break;
            case 'import':
                if (
                    launch.arguments.length !== 0
                    || Object.keys(launch.parameters || {}).some(name => name !== 'path')
                ) {
                    throw new Error('The local import request contains unexpected parameters.');
                }
                await installLocalModArchive(launch.parameters?.path);
                break;
            case 'launch': {
                const installationIndex = launch.arguments[0];
                if (!/^\d+$/.test(String(installationIndex || ''))) {
                    throw new Error('The installation index is invalid.');
                }
                const installationPath = path.join(app.getPath('userData'), `deltamod_system-${installationIndex}`);
                if (!fs.existsSync(installationPath)) {
                    throw new Error('The requested installation does not exist.');
                }
                writeFileAtomicSync(System.getSystemFile('_sysindex', true), String(installationIndex));
                app.relaunch();
                app.exit();
                break;
            }
            default:
                throw new Error('Unknown Deltamod Community protocol command.');
        }
    } catch (error) {
        page('main');
        errorWin(error);
        dialog.showErrorBox('Request failed', error.message);
    }
}

module.exports = {
    APPLICATION_SCHEME,
    handleProtocolLaunch,
    installLocalModArchive,
    parseLaunch,
    registerProtocolHandlers,
    registerProtocolSchemesAsPrivileged
};
