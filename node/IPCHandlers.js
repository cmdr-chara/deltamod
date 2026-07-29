const { app, BrowserWindow, ipcMain, dialog, shell, Notification, safeStorage } = require('electron');
const path = require('path');
const fs = require('fs');
const os = require('os');
const crypto = require('crypto');
const { spawn } = require('child_process');
const { Worker } = require('worker_threads');
const https = require('https');
const createDesktopShortcut = require('create-desktop-shortcuts');
const axios = require('axios');
var elevate = require('windows-elevate');
// Local modules
const KeyValue = require('./KeyValue');
const System = require('./System');
const { getSystemFile, getSystemFolder, getPacketDatabase, getSystemFolderOfIndex } = require('./System');
const Modstore = require('./Modstore');
const CMode = require('./ControllerMode');
const Updates = require('./Updates');
const GameDB = require('./GameDB');
const { createProgressModal, updateProgressModal } = require('./ProgressModal');
const GamePatching = require('./GamePatching');
const ProfileMigration = require('./ProfileMigration');
const UndertaleModTool = require('./UndertaleModTool');
const { downloadToFile } = require('./security/RemoteSecurity');
const { detectImageType } = require('./security/ImageSecurity');
const { resolveWithin } = require('./security/PathSecurity');
const { extractArchiveAtomic } = require('./security/ArchiveSecurity');
const { copyDirectoryAtomic } = require('./storage/StagedCopy');
const { readJsonSync, writeJsonAtomicSync, writeFileAtomicSync } = require('./storage/AtomicStore');
const Junction = require('./Junction');
const console = require('./Console');
const { PARTITION } = require('./Config');
const { page, getSharedVar, properRelaunch, getSteamDirectory, timeoutPromise } = require('./Utils');

// Using this fixes a vulnerability where attackers could freely download code
let updateStackInfo = null;

// --- IPC Helper Functions ---

function parseInstallationIndex(value) {
    const index = String(value ?? '');
    if (!/^\d+$/.test(index)) {
        const error = new Error('Invalid installation index.');
        error.code = 'INVALID_INSTALLATION_INDEX';
        throw error;
    }
    return index;
}

function getInstallationProfilePath(index) {
    return resolveWithin(app.getPath('userData'), `deltamod_system-${parseInstallationIndex(index)}`);
}

function parseUniqueFlagName(value) {
    const flag = String(value ?? '').toUpperCase();
    if (!/^[A-Z][A-Z0-9_]{0,63}$/.test(flag)) {
        const error = new Error('Invalid preference flag.');
        error.code = 'INVALID_PREFERENCE_FLAG';
        throw error;
    }
    return flag;
}

function parseExternalHttpsUrl(value, allowedHosts) {
    let parsed;
    try {
        parsed = new URL(String(value ?? ''));
    } catch {
        throw new Error('Invalid external URL.');
    }
    if (parsed.protocol !== 'https:' || parsed.username || parsed.password) {
        throw new Error('Only credential-free HTTPS URLs may be opened.');
    }
    const hostname = parsed.hostname.toLowerCase();
    if (allowedHosts && !allowedHosts.some(host => hostname === host || hostname.endsWith(`.${host}`))) {
        throw new Error(`External host is not approved: ${hostname}`);
    }
    return parsed.toString();
}

async function reorderInstalls() {
    const systemFiles = fs.readdirSync(app.getPath('userData')).filter(f => /^deltamod_system-\d+$/.test(f));
        
    var sorted = systemFiles.sort((a,b) => parseInt(a.split('-')[1]) - parseInt(b.split('-')[1]));
        
    for (let cNum = 0; cNum < sorted.length; cNum++) {
        const file = sorted[cNum];
        const oldPath = path.join(app.getPath('userData'), file);
        const newPath = path.join(app.getPath('userData'), `deltamod_system-${cNum}`);
        if (oldPath !== newPath) {
            fs.renameSync(oldPath, newPath);
            const cnamePath = path.join(newPath, '_cname');
            if (fs.existsSync(cnamePath) && fs.readFileSync(cnamePath, 'utf8').startsWith('Install #')) {
                writeFileAtomicSync(cnamePath, `Install #${cNum + 1}`);
            }
        }

        const storePath = path.join(newPath, 'store.json');
        const store = readJsonSync(storePath, {});
        if (typeof store.gamePath === 'string' && store.gamePath.endsWith('deltaruneInstall')) {
            console.log(`Updating game path for system index ${cNum} to reflect new index after deletion.`);
            store.gamePath = path.join(app.getPath('userData'), `deltamod_system-${cNum}`, 'deltaruneInstall');
            writeJsonAtomicSync(storePath, store);
        }
    }
}

async function dominantColor(imagePath) {
    try {
        const img = await loadImage(imagePath);
        // downscale for performance
        const w = 100;
        const h = Math.max(1, Math.round((img.height / img.width) * w));
        const canvas = createCanvas(w, h);
        const ctx = canvas.getContext('2d');
        ctx.drawImage(img, 0, 0, w, h);
        const data = ctx.getImageData(0, 0, w, h).data;

        const counts = new Map();
        let maxCount = 0;
        let dominant = null;

        // quantize to reduce unique colors (to nearest 16)
        for (let i = 0; i < data.length; i += 4) {
            const r = Math.round(data[i] / 16) * 16;
            const g = Math.round(data[i + 1] / 16) * 16;
            const b = Math.round(data[i + 2] / 16) * 16;
            const key = `${r},${g},${b}`;
            const v = (counts.get(key) || 0) + 1;
            counts.set(key, v);
            if (v > maxCount) {
                maxCount = v;
                dominant = { r, g, b };
            }
        }

        if (!dominant) return 'rgb(0, 0, 0)';
        return `rgb(${Math.max(dominant.r - 20, 0)}, ${Math.max(dominant.g - 20, 0)}, ${Math.max(dominant.b - 20, 0)})`;
    } catch (e) {
        console.log('dominantColor error', e);
        return 'rgb(0, 0, 0)';
    }
}

function obtainThemes() {
    const customThemeDir = path.join(app.getPath('userData'), 'customThemes');
    for (const directory of ['data', 'img', 'mus']) {
        fs.mkdirSync(path.join(customThemeDir, directory), { recursive: true });
    }

    const available = fs.readdirSync(path.join(__dirname, '..', 'web', 'themes', 'data'))
        .filter(f => f.endsWith('.theme.json'));
    const available2 = fs.readdirSync(path.join(customThemeDir, 'data'))
        .filter(f => f.endsWith('.theme.json'));

    const builtInThemes = available.map(f => ({
        ...JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'web', 'themes', 'data', f), 'utf8')),
        builtIn: true
    }));

    const customThemes = available2.map(f => ({
        ...JSON.parse(fs.readFileSync(path.join(customThemeDir, 'data', f), 'utf8')),
        builtIn: false
    })).filter(x => {
        const include = !available.map(n => n.replace('.theme.json', '')).includes(x.id);
        if (!include) console.log(`Custom theme "${x.id}" ignored because a built-in theme with the same ID exists.`);
        return include;
    });

    return [...builtInThemes, ...customThemes];
}

function validateDeltarune(deltapath) {
    if (typeof deltapath !== 'string' || !deltapath.trim() || deltapath === 'INVALID') return null;
    const keyItems = ['data.win'];
    const isValid = keyItems.every(item => {
        const exists = fs.existsSync(path.join(deltapath, item));
        if (!exists) console.log(`Missing key item: ${path.join(deltapath, item)}`);
        return exists;
    });
    return isValid ? deltapath : null;
}

async function getInstallations(suppressWarnings = false) {
    const userDataPath = app.getPath('userData');
    const systemFiles = fs.readdirSync(userDataPath).filter(file => /^deltamod_system-\d+$/.test(file));
    const installations = [];

    for (const file of systemFiles) {
        if (file.endsWith('unique')) continue;

        const installPath = path.join(userDataPath, file);
        const index = parseInt(file.split('-')[1], 10);
        const storeJSON = path.join(installPath, 'store.json');

        const storeData = readJsonSync(storeJSON, {});
        const deltaruneInstall = typeof storeData.gamePath === 'string'
            ? validateDeltarune(storeData.gamePath)
            : null;

        const cnamePath = path.join(installPath, '_cname');
        const issues = [];
        if (!fs.existsSync(storeJSON)) issues.push('Installation data store is missing');
        if (!deltaruneInstall) issues.push('Game directory or data.win is missing');
        if (!fs.existsSync(getPacketDatabase()) || !fs.statSync(getPacketDatabase()).isDirectory()) {
            issues.push('Community mod store is missing');
        }

        let commonName = `Install #${index + 1}`;
        try {
            commonName = fs.readFileSync(cnamePath, 'utf8');
        } catch(e) {
            writeFileAtomicSync(cnamePath, commonName);
        }

        installations.push({
            index,
            name: commonName,
            steam: KeyValue.readKVSOfIndex('isSteam', index) === true,
            pid: KeyValue.readKVSOfIndex('gamePid', index),
            appid: KeyValue.readKVSOfIndex('steamAppId', index),
            valid: issues.length === 0,
            canOpenInUndertaleModTool: process.platform === 'win32' && Boolean(deltaruneInstall),
            issues,
            repairActions: issues.length ? ['repair', 're-import', 'remove'] : []
        });
    }

    return installations;
}

async function precalculateHashes(root, operationId, onProgress) {
    if (!fs.existsSync(root) || !fs.lstatSync(root).isDirectory()) {
        throw new Error('The current game directory is unavailable.');
    }
    return new Promise((resolve, reject) => {
        const worker = new Worker(path.join(__dirname, 'workers', 'HashWorker.js'), {
            workerData: {
                root,
                operationId,
                cachePath: getSystemFile('_game-hashes.json', false)
            }
        });
        let settled = false;
        worker.on('message', message => {
            if (message?.error) {
                settled = true;
                const error = new Error(message.error.message);
                error.code = message.error.code;
                error.stack = message.error.stack;
                reject(error);
            } else if (message?.done) {
                settled = true;
                resolve(message);
            } else {
                onProgress?.(message);
            }
        });
        worker.on('error', error => {
            if (!settled) reject(error);
        });
        worker.on('exit', code => {
            if (!settled && code !== 0) reject(new Error(`Hash worker exited with code ${code}.`));
        });
    });
}

function copyRecursiveSync(src, dest) {
    if (fs.statSync(src).isDirectory()) {
        if (!fs.existsSync(dest)) fs.mkdirSync(dest, { recursive: true });
        for (const child of fs.readdirSync(src)) {
            copyRecursiveSync(path.join(src, child), path.join(dest, child));
        }
    } else {
        fs.copyFileSync(src, dest);
    }
}

function intoIM() {
    return {
        args: [
            ...process.argv.slice(1).filter(x => !/^deltamod(?:-community)?:\/\//i.test(x)),
            '---im'
        ]
    };
}

// --- IPC Registration ---

/**
 * Registers all IPC Handlers utilizing Dependency Injection to access required state safely.
 * @param {Object} context - The shared state and references from Runner.js
 */
module.exports = function registerIPCHandlers(context) {
    const { getWindow, isControllerMode, isDevToolsEnabled, errorWin, state } = context;
    const GameBanana = require('./GameBanana');
    const profileImports = new Map();
    const gameImports = new Map();
    // { getGBUIConf, collections }

    const undertaleModToolConfigPath = getSystemFile('undertale-mod-tool.json', true);
    const communityCliConfigPath = getSystemFile('deltamod-community-cli.json', true);

    function configuredUndertaleModToolExecutable() {
        const config = readJsonSync(undertaleModToolConfigPath, {});
        const candidates = [
            config?.executable,
            process.env.DELTAMOD_UMT_PATH
        ].filter(Boolean);

        for (const candidate of candidates) {
            try {
                return UndertaleModTool.validateExecutablePath(candidate);
            } catch {}
        }
        return null;
    }

    async function chooseUndertaleModToolExecutable() {
        if (process.platform !== 'win32') {
            const error = new Error('The WinUI UndertaleModTool integration is available only on Windows.');
            error.code = 'UMT_PLATFORM_UNSUPPORTED';
            throw error;
        }

        const options = {
            title: 'Choose UndertaleModTool',
            properties: ['openFile'],
            filters: [
                { name: 'UndertaleModTool executable', extensions: ['exe'] }
            ]
        };
        const owner = getWindow();
        const result = owner
            ? await dialog.showOpenDialog(owner, options)
            : await dialog.showOpenDialog(options);
        if (result.canceled || result.filePaths.length !== 1) return null;

        const executable = UndertaleModTool.validateExecutablePath(result.filePaths[0]);
        writeJsonAtomicSync(undertaleModToolConfigPath, {
            schemaVersion: 1,
            executable,
            updatedAt: new Date().toISOString()
        });
        return executable;
    }

    function configuredCommunityCliExecutable() {
        const config = readJsonSync(communityCliConfigPath, {});
        const candidates = [
            config?.executable,
            process.env.DELTAMOD_CLI_PATH,
            path.join(path.dirname(process.execPath), 'deltamod-community-cli.exe')
        ].filter(Boolean);

        for (const candidate of candidates) {
            try {
                return UndertaleModTool.validateCliExecutablePath(candidate);
            } catch {}
        }
        return null;
    }

    async function chooseCommunityCliExecutable() {
        if (process.platform !== 'win32') {
            const error = new Error('The UndertaleModTool bridge is currently available only on Windows.');
            error.code = 'UMT_PLATFORM_UNSUPPORTED';
            throw error;
        }

        const options = {
            title: 'Choose Deltamod Community CLI',
            properties: ['openFile'],
            filters: [
                { name: 'Deltamod Community CLI executable', extensions: ['exe'] }
            ]
        };
        const owner = getWindow();
        const result = owner
            ? await dialog.showOpenDialog(owner, options)
            : await dialog.showOpenDialog(options);
        if (result.canceled || result.filePaths.length !== 1) return null;

        const executable = UndertaleModTool.validateCliExecutablePath(result.filePaths[0]);
        writeJsonAtomicSync(communityCliConfigPath, {
            schemaVersion: 1,
            executable,
            updatedAt: new Date().toISOString()
        });
        return executable;
    }

    function requireTrustedRenderer(event) {
        const senderUrl = event.senderFrame?.url || event.sender?.getURL?.() || '';
        if (!senderUrl.startsWith('deltapack://web/')) {
            const error = new Error('IPC request was blocked because it did not originate from the application renderer.');
            error.code = 'UNTRUSTED_IPC_SENDER';
            throw error;
        }
    }

    function handle(channel, listener) {
        return ipcMain.handle(channel, (event, ...args) => {
            requireTrustedRenderer(event);
            return listener(event, ...args);
        });
    }

    async function officialProfileSummary() {
        const detected = ProfileMigration.detectOfficialProfile({
            appData: app.getPath('appData'),
            localAppData: process.env.LOCALAPPDATA,
            destinationRoot: app.getPath('userData')
        });
        if (!detected.exists) return detected;

        const summary = await ProfileMigration.inspectProfile(detected.sourceRoot, {
            destinationRoot: detected.destinationRoot,
            version: detected.version
        });
        let availableBytes = null;
        try {
            const stats = fs.statfsSync(path.dirname(detected.destinationRoot));
            availableBytes = Number(stats.bavail) * Number(stats.bsize);
        } catch {}

        return {
            ...detected,
            ...summary,
            availableBytes,
            requiredBytes: summary.totalBytes
                + Math.min(256 * 1024 * 1024, Math.ceil(summary.totalBytes * 0.05)),
            canImport: availableBytes === null
                || availableBytes >= summary.totalBytes
                    + Math.min(256 * 1024 * 1024, Math.ceil(summary.totalBytes * 0.05))
        };
    }

    handle('htmlAlert_outwin', (event, args) => {
        var title = args[0];
        var message = args[1];
        var buttons = args[2];

        var result = dialog.showMessageBoxSync(getWindow(), {
            title: title,
            message: message,
            buttons: buttons.map(b => b.text),
        });

        return result;
    });
    handle('isCMode', () => isControllerMode);
    handle('shouldGoIM', () => process.argv.includes('---im'));
    handle('diagnosticInfo', () => `Deltamod Community ${app.getVersion()} - Running on ${os.platform()} ${os.release()} - cmode ${isControllerMode ? 'on' : 'off'} - devtools ${isDevToolsEnabled ? 'enabled' : 'disabled'} - ${state.updateAvailable ? 'update available' : 'no update'}`);
    handle('isPackaged', () => app.isPackaged);
    handle('version', () => require('../package.json').version);
    handle('getOS', () => ({ platform: process.platform, release: os.release(), version: os.version() }));
    handle('isDevMode', () => process.argv.includes('--developer'));
    handle('getOfficialProfileSummary', async event => {
        requireTrustedRenderer(event);
        return officialProfileSummary();
    });
    handle('importOfficialProfile', async (event, args) => {
        requireTrustedRenderer(event);
        const detected = ProfileMigration.detectOfficialProfile({
            appData: app.getPath('appData'),
            localAppData: process.env.LOCALAPPDATA,
            destinationRoot: app.getPath('userData')
        });
        if (!detected.exists) {
            const error = new Error('No official Deltamod profile was found.');
            error.code = 'OFFICIAL_PROFILE_NOT_FOUND';
            throw error;
        }
        if ([...profileImports.values()].some(item => !item.signal.aborted)) {
            const error = new Error('A Deltamod profile import is already running.');
            error.code = 'PROFILE_IMPORT_RUNNING';
            throw error;
        }

        const requestedOperationId = String(args?.[0] || '');
        const operationId = /^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(requestedOperationId)
            ? requestedOperationId
            : crypto.randomUUID();
        const controller = new AbortController();
        profileImports.set(operationId, controller);

        try {
            const manifest = await ProfileMigration.importProfile({
                operationId,
                sourceRoot: detected.sourceRoot,
                destinationRoot: detected.destinationRoot,
                sourceVersion: detected.version,
                signal: controller.signal,
                migrateCredential: async encryptedCredential => {
                    if (!safeStorage.isEncryptionAvailable()) return null;
                    try {
                        return safeStorage.encryptString(safeStorage.decryptString(encryptedCredential));
                    } catch {
                        return null;
                    }
                },
                onProgress: progress => getWindow()?.webContents.send('profile-import-progress', progress)
            });
            return { operationId, manifest, restartRequired: true };
        } finally {
            profileImports.delete(operationId);
        }
    });
    handle('cancelOfficialProfileImport', (event, args) => {
        requireTrustedRenderer(event);
        const operationId = String(args?.[0] || '');
        const controller = profileImports.get(operationId);
        if (!controller) return false;
        controller.abort();
        return true;
    });
    handle('restartCommunity', event => {
        requireTrustedRenderer(event);
        app.relaunch({ args: process.argv.slice(1).filter(arg => !/^deltamod(?:-community)?:\/\//i.test(arg)) });
        app.exit();
    });

    handle('sampleError', () => errorWin('This is a sample error triggered from the renderer process.'));
    handle('log', (event, args) => console.rendererLog(args[1], args[2], args[0]));
    handle('showWindow', (event) => BrowserWindow.fromWebContents(event.sender).show());
    handle('minimizeMe', (event) => BrowserWindow.fromWebContents(event.sender)?.minimize());
    handle('toggleFullscreen', (event) => {
        const senderWin = BrowserWindow.fromWebContents(event.sender);
        if (senderWin) senderWin.setFullScreen(!senderWin.isFullScreen());
    });
    handle('cmode-on', () => {
        app.relaunch({ args: [...process.argv.slice(1).filter(arg => arg !== '-controller' && !/^deltamod(?:-community)?:\/\//i.test(arg)), '-controller'] });
        app.exit(0);
    });
    handle('cmode-off', () => {
        app.relaunch({ args: process.argv.slice(1).filter(arg => arg !== '-controller' && !/^deltamod(?:-community)?:\/\//i.test(arg)) });
        app.exit(0);
    });
    handle('rebootDev', async () => {
        if (process.argv.includes('--developer')) return false;
        const existingArgs = process.argv.slice(1).filter(a =>
            !a.startsWith('---system_index=')
            && a !== '---initialize_deltamod'
            && !/^deltamod(?:-community)?:\/\//i.test(a)
        );
        app.relaunch({ args: [...existingArgs, '--developer'] });
        app.exit(0);
    });

    // Themes
    handle('chooseTheme', async () => {
        const win = getWindow();
        const themesDir = path.join(__dirname, '..', 'web', 'themes');
        const themeObjects = fs.readdirSync(themesDir)
            .filter(f => f.endsWith('.theme.json'))
            .map(f => JSON.parse(fs.readFileSync(path.join(themesDir, f), 'utf8')));

        const choice = dialog.showMessageBoxSync(win, {
            type: 'question',
            title: 'Select a theme',
            message: 'Select a theme from the list below:',
            buttons: [...themeObjects.map(t => t.name), 'Cancel'],
            cancelId: themeObjects.length
        });
        
        if (choice === themeObjects.length) return;
        
        const themeId = themeObjects[choice].id;
        writeFileAtomicSync(System.getSystemFile('_theme', true), themeId);
        if(win) win.webContents.send('themeChange');
    });

    handle('setTheme', (event, args) => {
        const themeId = String(args[0] || '');
        if (!obtainThemes().some(theme => theme.id === themeId)) throw new Error('Unknown theme.');
        writeFileAtomicSync(System.getSystemFile('_theme', true), themeId);
        return true;
    });
    handle('getThemes', () => obtainThemes());
    handle('getTheme', async () => {
        const themeHost = System.getSystemFile('_theme', true);
        let themeId = 'home';
        
        if (fs.existsSync(themeHost)) {
            themeId = fs.readFileSync(themeHost, 'utf8').trim();
            const validThemes = obtainThemes();
            if (!validThemes.find(t => t.id === themeId)) themeId = 'home';
        }
        
        writeFileAtomicSync(themeHost, themeId);
        return themeId;
    });

    handle('importTheme', async () => {
        const win = getWindow();
        const musicPath = (await dialog.showOpenDialog(win, { title: 'Select your music file', filters: [{ name: 'Song files', extensions: ['mp3', 'ogg'] }] })).filePaths[0];
        const bgPath = (await dialog.showOpenDialog(win, { title: 'Select your background image', filters: [{ name: 'Image files', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif' ] }] })).filePaths[0];
        if (!musicPath || !bgPath) return;
        const musicExtension = path.extname(musicPath).toLowerCase();
        const imageExtension = path.extname(bgPath).toLowerCase();
        if (!['.mp3', '.ogg'].includes(musicExtension)) throw new Error('Unsupported theme audio type.');
        if (!['.png', '.jpg', '.jpeg', '.webp', '.gif'].includes(imageExtension)) {
            throw new Error('Unsupported theme image type.');
        }
        const detectedImage = await detectImageType(bgPath);
        const expectedImage = imageExtension === '.jpg' ? 'jpeg' : imageExtension.slice(1);
        if (detectedImage !== expectedImage) throw new Error('Theme image signature does not match its extension.');

        const randomSeed = Math.random().toString(36).substring(2, 15);
        const themeId = `custom_${randomSeed}`;
        const themeName = `Custom Theme #${randomSeed.substring(0, 5).toUpperCase()}`;
        const customThemesDir = path.join(app.getPath('userData'), 'customThemes');
        for (const directory of ['mus', 'img', 'data']) {
            fs.mkdirSync(path.join(customThemesDir, directory), { recursive: true });
        }

        fs.copyFileSync(musicPath, path.join(customThemesDir, 'mus', `${themeId}${musicExtension}`));
        fs.copyFileSync(bgPath, path.join(customThemesDir, 'img', `${themeId}${imageExtension}`));

        const config = {
            name: themeName,
            background: `${themeId}${imageExtension}`,
            description: `This is a custom theme by the user.`,
            mainSong: `${themeId}${musicExtension}`,
            id: themeId,
            musicTrack: "Custom music",
            color: await dominantColor(bgPath)
        };

        writeJsonAtomicSync(path.join(customThemesDir, 'data', `${themeId}.theme.json`), config);
        page('themesel');
    });

    handle('renameCustomTheme', async (event, args) => {
        const [themeId, newName, newDesc] = args;
        if (!/^custom_[a-z0-9]+$/i.test(String(themeId))) throw new Error('Invalid custom theme identifier.');
        const customDataRoot = path.join(app.getPath('userData'), 'customThemes', 'data');
        const customJSON = resolveWithin(customDataRoot, `${themeId}.theme.json`, { mustExist: true });
        const themeConfig = readJsonSync(customJSON, null);
        if (!themeConfig) throw new Error('Custom theme data is invalid.');
        themeConfig.name = String(newName || '').trim().slice(0, 100) || 'Custom theme';
        themeConfig.description = String(newDesc || '').trim().slice(0, 500);
        writeJsonAtomicSync(customJSON, themeConfig);
        return true;
    });

    handle('deleteCustomTheme', async (event, args) => {
        const themeId = String(args[0] || '');
        if (!/^custom_[a-z0-9]+$/i.test(themeId)) throw new Error('Invalid custom theme identifier.');
        const customRoot = path.join(app.getPath('userData'), 'customThemes');
        const customJSON = resolveWithin(path.join(customRoot, 'data'), `${themeId}.theme.json`, { mustExist: true });
        const themeConfig = readJsonSync(customJSON, {});
        for (const [directory, file] of [['img', themeConfig.background], ['mus', themeConfig.mainSong]]) {
            if (typeof file !== 'string') continue;
            try {
                const asset = resolveWithin(path.join(customRoot, directory), file, { mustExist: true });
                await fs.promises.rm(asset, { force: true });
            } catch {}
        }
        await fs.promises.rm(customJSON, { force: true });
        return true;
    });

    // Sponsors
    handle('setSponsor', async () => {
        const win = getWindow();
        const base = path.join(__dirname, '..', 'web', 'views', 'patching', 'sponsors');
        let sponsors = fs.readdirSync(base);
        if (Math.random() >= 0.08) sponsors = sponsors.filter(s => s !== 'musical');

        const buttons = sponsors.map(s => JSON.parse(fs.readFileSync(path.join(base, s, 'config.sponsor.json'), 'utf8')).name);

        const choice = dialog.showMessageBoxSync(win, {
            type: 'question',
            title: 'Select a patching character',
            message: 'Select a patching character from the list below:',
            buttons: [...buttons, 'Cancel'],
        });

        if (choice === buttons.length) return;
        writeFileAtomicSync(System.getSystemFile('_sponsor', true), sponsors[choice]);
    });
    handle('getSponsor', () => {
        const sponsorHost = System.getSystemFile('_sponsor', true);
        if (fs.existsSync(sponsorHost)) return fs.readFileSync(sponsorHost, 'utf8');
        writeFileAtomicSync(sponsorHost, 'cd');
        return 'cd';
    });

    // GameBanana Auth & API
    handle('loginGamebanana', async () => {
        if (!safeStorage.isEncryptionAvailable()) {
            dialog.showMessageBoxSync({
                type: 'error',
                title: 'Secure storage unavailable',
                message: 'GameBanana login cannot be saved because encrypted credential storage is unavailable on this system.',
            });
            return false;
        }
        const token = await GameBanana.obtainLogin();
        const file = getSystemFile('bananapwd', true);
        writeFileAtomicSync(file, safeStorage.encryptString(token));
        return true;
    });
    handle('logoutGamebanana', async () => {
        return GameBanana.clearLoginSession();
    });
    handle('eraseGamebananaCache', () => GameBanana.clearCache());
    handle('leaveCommentGamebanana', async (event, args) => {
        const uiconf = await GameBanana.getGBUIConf();
        if (!(uiconf._idMemberRow > 0)) {
            const error = new Error('Log in to GameBanana before posting a comment.');
            error.code = 'GAMEBANANA_LOGIN_REQUIRED';
            throw error;
        }
        return await GameBanana.leaveComment(args[0], args[1], args[2]);
    });
    handle('gbLikeMod', async (event, args) => {
        const uiconf = await GameBanana.getGBUIConf();
        if (uiconf._idMemberRow > 0) return await GameBanana.likeMod(args[0], args[1]);
    });
    handle('validateGamebananaToken', async () => (await GameBanana.getGBUIConf())._idMemberRow > 0);
    handle('getGamebananaPic', async () => (await GameBanana.getGBUIConf())._sAvatarUrl);
    handle('getGamebananaID', async () => (await GameBanana.getGBUIConf())._idMemberRow);
    handle('getGamebananaUserinfo', async () => {
        try {
            const id = (await GameBanana.getGBUIConf())._idMemberRow;
            if (id <= 0) return { loggedIn: false };
            const profile = await axios.get(`https://gamebanana.com/apiv11/Member/${id}/ProfilePage`);
            return { ...profile.data, loggedIn: true };
        } catch (error) {
            console.error('Error fetching GameBanana user info:', error);
            return { loggedIn: false };
        }
    });
    
    // Mod Management
    handle('importMod', async () => {
        const win = getWindow();
        const { canceled, filePaths } = await dialog.showOpenDialog(win, {
            properties: ['openFile'],
            filters: [{ name: 'Deltamod compatible archive', extensions: ['zip', '7z', 'tar.gz', 'lzma'] }]
        });
        if (!canceled && filePaths?.[0]) return Modstore.importMod(filePaths[0]);
        return false;
    });
    handle('removeMod', async (event, args) => await Modstore.removeModSafe(args[0]));
    handle('toggleModState', (event, args) => {
        const enabled = KeyValue.readKVS("enabledMods", []);
        KeyValue.setKVS("enabledMods", args[1] ? [...enabled, args[0]] : enabled.filter(x => x !== args[0]));
    });
    handle('getModState', (event, args) => KeyValue.readKVS("enabledMods", []).includes(args[0]));
    handle('getModList', () => {
        const { modList, errors } = Modstore.modList();
        const edition = KeyValue.readKVS('gamePid');
        const processedList = modList.map(mod => {
            mod.isIncompatible = false;
            if (mod._incompatibleHASH) {
                mod.isIncompatible = true;
                mod.incompatibilityReason = 'Mismatching hashes for files: ' + mod._hashDifferentFiles.map(file => '"' + file + '"').join(', ');
                delete mod._incompatibleHASH;
            }
            if (mod.game !== edition) {
                mod.isIncompatible = true;
                mod.incompatibilityReason = 'Mod is for ' + GameDB.getGameById(mod.game)?.name + ' but your current game is ' + GameDB.getGameById(edition)?.name;
            }
            return mod;
        });
        return { modList: processedList, errors };
    });
    handle('getModListFull', () => Modstore.modList());
    handle('howManyMods', () => Modstore.howmany());
    handle('dlmodURL', async (event, args) => {
        const [url, queryme, modid, modmodel] = args;
        const requestId = String(queryme ?? '');
        if (!/^[a-z0-9]{1,32}$/i.test(requestId)) throw new Error('Invalid download operation identifier.');
        try {
            return await Modstore.downloadModFromURL(url, (progress, downloaded) => {
                event.sender.send('dlmodURL-progress', { progress, downloaded, queryme: requestId, error: false });
            }, modid, modmodel);
        } catch (error) {
            event.sender.send('dlmodURL-progress', {
                progress: 0,
                downloaded: 0,
                queryme: requestId,
                error: true,
                message: error.message
            });
            throw error;
        }
    });
    handle('setModVariant', (event, args) => {
        const modFolder = Modstore.resolveModFolder(String(args[1]), true);
        const variant = String(args[0] || '');
        if (!variant || path.isAbsolute(variant) || variant.split(/[\\/]/).includes('..')) {
            throw new Error('Invalid mod variant path.');
        }
        writeFileAtomicSync(path.join(modFolder, '__variant'), variant);
        return true;
    });
    handle('getModImage', (event, args) => Modstore.getModImage(args[0]));

    // Game Operations
    handle('precalcGameHashes', event => {
        const operationId = crypto.randomUUID();
        return precalculateHashes(
            getSystemFolder('deltaruneInstall'),
            operationId,
            progress => event.sender.send('hash-progress', progress)
        );
    });
    handle('getCurrentGameInfo', () => GameDB.getGameById(KeyValue.readKVS('gamePid')));
    handle('getGameInfo', (event, args) => GameDB.getGameById(args[0]));
    handle('getAvailableGames', () => GameDB.getGames());
    handle('loadedDeltarune', () => {
        try {
            const kvs = KeyValue.readKVS('gamePid');
            const gameInfo = GameDB.getGameById(kvs);
            return { loaded: fs.existsSync(path.join(KeyValue.readKVS('gamePath'), gameInfo.exeName)), path: kvs };
        } catch {
            return { loaded: false, path: "" };
        }
    });

    handle('startGame', (event, args) => ipcMain.emit('startGame', event, args));
    ipcMain.on('startGame', () => {
        const win = getWindow();
        const installPath = KeyValue.readKVS('gamePath');

        if (win) {
            win.hide();
            win.webContents.send('audio', false);
        }

        if (KeyValue.readKVS('isSteam')) {
            const steamAppId = String(KeyValue.readKVS('steamAppId') ?? '');
            if (!/^\d{1,12}$/.test(steamAppId)) {
                errorWin('The Steam application ID stored for this installation is invalid.');
                if (win) {
                    win.show();
                    win.webContents.send('audio', true);
                }
                return false;
            }
            shell.openExternal(`steam://rungameid/${steamAppId}`);
            app.quit();
            return process.exit(0);
        }

        const gameConfig = GameDB.getGameById(KeyValue.readKVS('gamePid'));
        const exePath = path.join(installPath, gameConfig.exeName);
        if (!fs.existsSync(exePath)) {
            errorWin('Could not find executable to run.');
            if (win) {
                win.show();
                win.webContents.send('audio', true);
            }
            return false;
        }

        if (isControllerMode) CMode.stop();

        const configuredLauncher = KeyValue.readKVS('linuxLauncher', null);
        const launcher = process.platform === 'linux'
            ? (
                configuredLauncher
                && typeof configuredLauncher === 'object'
                && typeof configuredLauncher.command === 'string'
                && configuredLauncher.command.trim()
                    ? {
                        command: configuredLauncher.command.trim(),
                        args: Array.isArray(configuredLauncher.args)
                            ? configuredLauncher.args.map(arg => String(arg).replaceAll('{exe}', exePath))
                            : [exePath]
                    }
                    : { command: 'wine', args: [exePath] }
            )
            : { command: exePath, args: [] };

        let finalized = false;
        const finalizeGame = () => {
            if (finalized) return;
            finalized = true;
            try { GamePatching.restore(installPath); } catch (e) { console.error('Failed to restore originals:', e); }
            if (isControllerMode) CMode.start();
            if (win) {
                win.show();
                win.webContents.send('audio', true);
                win.webContents.send('page', 'main');
            }
        };

        const gameProcess = spawn(launcher.command, launcher.args, {
            cwd: path.dirname(exePath),
            windowsHide: true,
            shell: false
        });
        gameProcess.once('error', error => {
            errorWin(`Could not start the game: ${error.message}`);
            finalizeGame();
        });
        gameProcess.once('close', finalizeGame);

        return true;
    });

    handle('gamebanana_getCollections', async () => {
        var res = await GameBanana.collections.list();
        return (typeof res === 'object' && Array.isArray(res)) ? res.map(c => ({
            id: c._idRow,
            name: c._sName,
        })) : res;
    });

    handle('gamebanana_createCollection', async (event, args) => {
        return await GameBanana.collections.create(args[0]);
    });

    handle('gamebanana_deleteCollection', async (event, args) => {
        return await GameBanana.collections.delete(args[0]);
    });

    handle('gamebanana_importToCollection', async (event, args) => {
        var pkgDB = getPacketDatabase();

        var gbMods = args[1];

        const skippedMods = [];
        
        for (const mod of gbMods) {
            const added = await GameBanana.collections.add(args[0], mod.id, mod.model);
            if (!added.success) {
                skippedMods.push({
                    name: mod.name,
                    pid: mod.pid,
                    reason: 'Failed to add to backup (API error)',
                    api: added.error
                });
            }
        }

        return { done: true, skippedMods };
    });

    handle('gamebanana_downloadAllInCollection', async (event, args) => {
        var mods = await GameBanana.collections.inspect(args[0]);

        var pwin = createProgressModal();

        for (const mod of mods) {

            var todownload = mod.files[0];
            if (mod.files.length > 1) {
                var result = dialog.showMessageBoxSync({
                    type: 'warning',
                    title: 'Multiple versions found',
                    message: `Multiple files for "${mod.mod}" were found. Choose which file to download`,
                    buttons: [...mod.files.map(f => f.filename), 'Cancel'],
                });

                if (result !== mod.files.length) {
                    todownload = mod.files[result];
                }
            }

            var dlpath = path.join(app.getPath('downloads'), Math.random().toString(36).substring(2, 15) + '.' + todownload.filename.split('.')[todownload.filename.split('.').length - 1]);

            await downloadToFile(todownload.url, dlpath, {
                maximumBytes: 2 * 1024 * 1024 * 1024,
                onProgress: progress => {
                    if (pwin && progress.total) {
                        updateProgressModal(pwin, null, progress.completed / progress.total, 'Downloading mod');
                    }
                }
            });

            await Promise.race([
                Modstore.importMod(dlpath, 'donothing', mod.mod, mod.model),
                new Promise(resolve => setTimeout(resolve, 10000))
            ]);

            fs.unlinkSync(dlpath);
        }

        pwin.close();

        app.relaunch({ args: process.argv.slice(1).filter(arg => arg !== '-controller' && !/^deltamod(?:-community)?:\/\//i.test(arg)).concat(isControllerMode ? ['-controller'] : []) });
        app.exit(0);

    });

    handle('patchAndRun', async (event, args) => {
        const win = getWindow();
        try {
            if (!Array.isArray(args?.[0]) || args[0].length > 1000) {
                throw new Error('The selected mod list is invalid.');
            }
            const selectedMods = [...new Set(args[0].map(value => String(value)))];
            if (selectedMods.some(value => !value || value.length > 256 || /[\r\n\0]/.test(value))) {
                throw new Error('The selected mod list contains an invalid identifier.');
            }
            const baking = args[1] === 'baker';
            const pathname = KeyValue.readKVS('gamePath');
            if (!pathname) return dialog.showErrorBox('Error', 'Please import a Deltarune install first.');

            GamePatching.restore(pathname);

            let mods = fs.readdirSync(getPacketDatabase()).filter(f => fs.existsSync(path.join(getPacketDatabase(), f, '__deltaID.json'))).map(f => {
                const dataPath = path.join(getPacketDatabase(), f, '__deltaID.json');
                const data = JSON.parse(fs.readFileSync(dataPath, 'utf8'));
                if (selectedMods.includes(String(data.uniqueId))) {
                    data.new = false;
                    writeJsonAtomicSync(dataPath, data);
                }
                return data;
            });

            const log = await GamePatching.startGamePatch(pathname, getPacketDatabase(), selectedMods, (log) => {
                win?.webContents.send('gplog', {log, percent: -1});
            }, (percent) => {
                win?.webContents.send('gplog', {log: '', percent});
            }).catch(err => {
                return { patched: false, log: `Error during patching: ${err.message}` };
            });

            console.log('got ' + JSON.stringify(log));

            if (!log.patched) {
                var res = dialog.showMessageBoxSync(win, {
                    type: 'error',
                    title: 'Patching failed',
                    message: `Patching failed with the following error:\n\n${log.log}`,
                    buttons: ['Save full log to Desktop', 'OK']
                });
                if (res === 0) {
                    const desktopPath = app.getPath('desktop');
                    const logFilePath = path.join(desktopPath, `deltamod_patch_log_${Date.now()}.txt`);
                    writeFileAtomicSync(logFilePath, "SHORTENED LOG: " + log.log + "\n\nFULL LOG: \n\n" + log.fullLog);
                    dialog.showMessageBoxSync(win, {
                        type: 'info',
                        title: 'Logs saved',
                        message: `Logs have been saved to your Desktop:\n${logFilePath}`,
                        buttons: ['OK']
                    });
                }
                win?.webContents.send('audio', true);
                page('main');
                return false;
            }

            const notif = new Notification({ title: 'Patch complete!', body: 'The game has been patched successfully!' });
            notif.on('click', () => {
                const currentWin = getWindow();
                if (!currentWin) return;
                if (currentWin.isMinimized()) currentWin.restore();
                currentWin.show();
                currentWin.focus();
                currentWin.setAlwaysOnTop(true);
                setTimeout(() => currentWin.setAlwaysOnTop(false), 100);
            });
            notif.show();

            state.callbackNPS = () => ipcMain.emit('startGame', null, []);
            
            state.callbackNPSPassWith = [pathname];
            if (win) win.webContents.send('finishedPatch', mods);
        } catch (err) {
            if (err.message && err.message.includes('Restarting')) return false;
            errorWin(`Couldn't patch and run game: ${err.message}`);
            return false;
        }
    });

    handle('downloadGame', async (event, args) => {
        const win = getWindow();
        const dataFeat = GameDB.getFeatInfo(args[0], 'autodownload').data;
        const deltaruneUrl = await require(`./DownloadUtilities/${dataFeat.pluginName}`).run(args[0], dataFeat);
        const modal = createProgressModal();
        const destPath = path.join(System.getTemporary(), "deltaruneGAME.zip");
        let createdExtractRoot = null;

        try {
            await fs.promises.rm(destPath, { force: true });
            await downloadToFile(deltaruneUrl, destPath, {
                maximumBytes: 8 * 1024 * 1024 * 1024,
                allowedHosts: hostname => {
                    const normalized = hostname.toLowerCase().replace(/\.$/, '');
                    return [
                        'itch.io',
                        'hwcdn.net',
                        'gamejolt.com',
                        'gamejolt.net',
                        'gjcdn.net'
                    ].some(host => normalized === host || normalized.endsWith(`.${host}`));
                },
                onProgress: progress => {
                    if (progress.total) {
                        updateProgressModal(modal, win, progress.completed / progress.total, 'Downloaded');
                    }
                }
            });

            if (win) win.setProgressBar(0);
            let extractPath = path.join(System.getTemporary(), `game_ext_${Date.now()}`);
            createdExtractRoot = extractPath;
            await extractArchiveAtomic(destPath, extractPath, {
                limits: {
                    maxArchiveBytes: 8 * 1024 * 1024 * 1024,
                    maxExpandedBytes: 16 * 1024 * 1024 * 1024,
                    maxFiles: 100_000
                }
            });

            const files = fs.readdirSync(extractPath);
            if (files.length === 1) extractPath = path.join(extractPath, files[0]);

            return extractPath;
        } catch (err) {
            if (createdExtractRoot) {
                await fs.promises.rm(createdExtractRoot, { recursive: true, force: true });
            }
            throw err;
        } finally {
            await fs.promises.rm(destPath, { force: true });
            if (!modal.isDestroyed()) modal.close();
        }
    });

    // Install Management
    handle('getSystemIndex', () => {
        const overridePath = getSystemFile('_sysindex', true);
        return fs.existsSync(overridePath) ? fs.readFileSync(overridePath, 'utf8') : 0;
    });
    handle('getMaxExistingIndex', () => {
        try {
            const systemFiles = fs.readdirSync(app.getPath('userData')).filter(f => f.startsWith('deltamod_system-'));
            let maxIndex = 0;
            const invalidInstalls = [];
            for (const file of systemFiles) {
                const index = file.split('-')[1];
                if (index === 'unique') continue;
                const store = readJsonSync(path.join(app.getPath('userData'), file, 'store.json'), {});
                if (!validateDeltarune(store.gamePath)) {
                    invalidInstalls.push(index);
                    continue;
                }
                maxIndex = Math.max(maxIndex, parseInt(index, 10));
            }
            return [maxIndex, invalidInstalls];
        } catch (err) { return [0, []]; }
    });
    handle('getInstallations', async () => await getInstallations());
    handle('setInstallationCName', (event, args) => {
        requireTrustedRenderer(event);
        const index = parseInstallationIndex(args?.[0]);
        const name = String(args?.[1] ?? '').replace(/[\r\n\0]/g, ' ').trim();
        if (!name || name.length > 80) throw new Error('Installation names must contain 1 to 80 characters.');
        const installationPath = getInstallationProfilePath(index);
        if (!fs.existsSync(installationPath)) throw new Error('Installation profile does not exist.');
        writeFileAtomicSync(path.join(installationPath, '_cname'), name);
        return true;
    });
    handle('changeSystemIndex', (event, args) => {
        requireTrustedRenderer(event);
        const index = parseInstallationIndex(args?.[0]);
        if (!fs.existsSync(getInstallationProfilePath(index))) throw new Error('Installation profile does not exist.');
        writeFileAtomicSync(getSystemFile('_sysindex', true), index);
        app.relaunch(intoIM());
        app.exit();
    });
    handle('getEditionByIndex', (event, args) => {
        requireTrustedRenderer(event);
        return KeyValue.readKVSOfIndex('gamePid', parseInstallationIndex(args?.[0])) || "Unknown";
    });
    
    handle('createNewInstallation', async (event, args) => {
        // arguments
        const win = getWindow();
        const steam = args[0] === 'steam';
        const isFromLocate = args[1] === 'locate';
        const specifiedLocatePath = isFromLocate ? args[2] : null;
        const fromIM = args[3];
        let selectedGame = args[4];
        let copyToDMod = args[5] == 'copy';

        let i = 0;
        fs.readdirSync(app.getPath('userData')).filter(f => f.startsWith('deltamod_system-')).forEach(file => {
            const idx = file.split('-')[1];
            if (idx !== 'unique') i = Math.max(i, parseInt(idx, 10));
        });
        i = (isFromLocate && !fromIM) ? parseInt(System.getCurrentSystemIndex()) : i + 1;
        
        let sourcePath = specifiedLocatePath;
        let chosenEdition;

        if (!selectedGame) {
            const games = GameDB.getGames();
            const response = dialog.showMessageBoxSync({ type: 'question', title: 'Choose game', message: 'Select imported game:', buttons: games.map(x => x.name) });
            selectedGame = games[response].id;
        }

        const gameInfo = GameDB.getGameById(selectedGame);

        if (steam) {
            var steamdata = gameInfo.availableFeatures.find(e => e.feat == 'steam').data;
            var steamPath = path.join(getSteamDirectory(dialog), steamdata.folder);

            sourcePath = steamPath;
            chosenEdition = { appid: steamdata.appid };

            var el = (await getInstallations(true)).find(inst => inst.appid == chosenEdition.appid);
            if (el) {
                dialog.showErrorBox('Duplicate Steam install', 'You can\'t import the same Steam installation twice. Looks like you already have this game imported as "' + el.name + '".');
                return false;
            }
        }

        if (!validateDeltarune(sourcePath)) {
            dialog.showErrorBox('Invalid folder', steam ? 'Game missing from Steam library.' : 'Invalid game installation.');
            return false;
        }

        if (!fs.existsSync(path.join(sourcePath, gameInfo.exeName))) {
            dialog.showErrorBox('Invalid install', `Missing executable: ${gameInfo.exeName}`);
            return false;
        }

        const installationPath = path.join(app.getPath('userData'), `deltamod_system-${i}`);
        if (!fs.existsSync(installationPath)) {
            console.log('Initialized sysdir for new installation at index', i);
            fs.mkdirSync(installationPath, { recursive: true });
        }

        let destPath;
        let copiedToCommunity = false;

        if (copyToDMod) {
            destPath = path.join(installationPath, 'deltaruneInstall');
            console.log(`Copying files from ${sourcePath} to Community storage (${destPath})...`);
            const operationId = crypto.randomUUID();
            const controller = new AbortController();
            gameImports.set(operationId, controller);
            try {
                await copyDirectoryAtomic({
                    operationId,
                    source: sourcePath,
                    destination: destPath,
                    signal: controller.signal,
                    onProgress: progress => win?.webContents.send('game-import-progress', progress)
                });
                copiedToCommunity = true;
            }
            catch (err) {
                dialog.showErrorBox(
                    'Copy failed',
                    `${err.message}\n\nFailed operation: ${err.details?.operation || 'copy'}`
                    + `${err.details?.source ? `\nSource: ${err.details.source}` : ''}`
                    + `${err.details?.destination ? `\nDestination: ${err.details.destination}` : ''}`
                );
                console.error('Error during copy:', err);
                return false;
            } finally {
                gameImports.delete(operationId);
            }

            console.log('Copy completed successfully.');
        }
        else {
            destPath = sourcePath;
        }

        try {
            const storePath = path.join(installationPath, 'store.json');
            writeJsonAtomicSync(storePath, {
                ...readJsonSync(storePath, {}),
                version: `DELTAMOD_DATA_${require('../package.json').version}`,
                loadedDeltarune: true,
                gamePath: destPath,
                gamePid: selectedGame,
                deltaruneEdition: 'rem',
                enabledMods: [],
                isSteam: steam,
                steamAppId: steam ? chosenEdition.appid : ''
            });

            page(fromIM ? "installmanager" : "main");
            return true;
        } catch (err) {
            if (copiedToCommunity) {
                try { fs.rmSync(destPath, { recursive: true, force: true }); } catch {}
            }
            dialog.showErrorBox('Import failed', `Failed: ${err.message}\n\n${err.stack}`);
            return false;
        }
    });

    handle('cancelGameImport', (event, args) => {
        requireTrustedRenderer(event);
        const operationId = String(args?.[0] || '');
        const controller = gameImports.get(operationId);
        if (!controller) return false;
        controller.abort();
        return true;
    });

    handle('repairInstallation', (event, args) => {
        requireTrustedRenderer(event);
        const index = String(args?.[0] || '');
        if (!/^\d+$/.test(index)) throw new Error('Invalid installation index.');
        const installationPath = path.join(app.getPath('userData'), `deltamod_system-${index}`);
        const storePath = path.join(installationPath, 'store.json');
        const store = readJsonSync(storePath, {});
        const candidateGamePath = typeof store.gamePath === 'string'
            ? store.gamePath
            : path.join(installationPath, 'deltaruneInstall');

        try { GamePatching.restore(candidateGamePath); } catch (error) {
            return { repaired: false, issues: [`Patch recovery failed: ${error.message}`] };
        }
        fs.mkdirSync(getPacketDatabase(), { recursive: true });

        const issues = [];
        if (!fs.existsSync(storePath)) issues.push('Installation data store is missing');
        if (!validateDeltarune(candidateGamePath)) issues.push('Game directory or data.win is missing');
        return { repaired: issues.length === 0, issues };
    });

    handle('reimportInstallation', async (event, args) => {
        requireTrustedRenderer(event);
        const index = String(args?.[0] || '');
        if (!/^\d+$/.test(index)) throw new Error('Invalid installation index.');
        const installationPath = path.join(app.getPath('userData'), `deltamod_system-${index}`);
        if (!fs.existsSync(installationPath)) throw new Error('Installation profile does not exist.');

        const storePath = path.join(installationPath, 'store.json');
        const store = readJsonSync(storePath, {});
        const gameInfo = GameDB.getGameById(store.gamePid);
        if (!gameInfo) throw new Error('The installation does not identify a supported game.');

        const selection = await dialog.showOpenDialog(getWindow(), {
            title: 'Choose a clean game installation to re-import',
            properties: ['openDirectory']
        });
        if (selection.canceled || !selection.filePaths[0]) return { cancelled: true };
        const sourcePath = selection.filePaths[0];
        if (!validateDeltarune(sourcePath) || !fs.existsSync(path.join(sourcePath, gameInfo.exeName))) {
            throw new Error(`The selected folder is not a valid ${gameInfo.name} installation.`);
        }

        const defaultDestination = path.join(installationPath, 'deltaruneInstall');
        const destination = fs.existsSync(defaultDestination)
            ? path.join(installationPath, `deltaruneInstall-reimport-${Date.now()}`)
            : defaultDestination;
        const operationId = crypto.randomUUID();
        const controller = new AbortController();
        gameImports.set(operationId, controller);
        let copied = false;
        try {
            await copyDirectoryAtomic({
                operationId,
                source: sourcePath,
                destination,
                signal: controller.signal,
                onProgress: progress => getWindow()?.webContents.send('game-import-progress', progress)
            });
            copied = true;
            writeJsonAtomicSync(storePath, {
                ...store,
                version: `DELTAMOD_DATA_${require('../package.json').version}`,
                loadedDeltarune: true,
                gamePath: destination
            });
            return { repaired: true, operationId, destination };
        } catch (error) {
            if (copied) {
                try { fs.rmSync(destination, { recursive: true, force: true }); } catch {}
            }
            throw error;
        } finally {
            gameImports.delete(operationId);
        }
    });

    handle('isCurrentIndexSteam', () => KeyValue.readKVSOfIndex('isSteam', parseInt(System.getCurrentSystemIndex())));
    handle('removeSteamIntegration', () => {
        const index = parseInt(System.getCurrentSystemIndex());
        
        if (KeyValue.readKVSOfIndex('gamePath', index).endsWith('deltaruneInstall')) {
            Junction.deleteJunction(KeyValue.readKVSOfIndex('originalSteamPath', index));
        }

        KeyValue.setKVSOfIndex('isSteam', false, index);
        KeyValue.setKVSOfIndex('steamAppId', "", index);
        app.relaunch(properRelaunch());
        app.exit();
    });

    handle('deleteSystemIndex', (event, args) => {
        requireTrustedRenderer(event);
        const index = parseInstallationIndex(args?.[0]);
        const numericIndex = Number.parseInt(index, 10);
        const gamePath = KeyValue.readKVSOfIndex('gamePath', numericIndex, '');
        if (KeyValue.readKVSOfIndex('isSteam', numericIndex) && typeof gamePath === 'string' && gamePath.endsWith('deltaruneInstall')) {
            Junction.deleteJunction(KeyValue.readKVSOfIndex('originalSteamPath', numericIndex));
        }

        const pathToDelete = getInstallationProfilePath(index);
        if (fs.existsSync(pathToDelete)) fs.rmSync(pathToDelete, { recursive: true, force: true });

        reorderInstalls();

        writeFileAtomicSync(getSystemFile('_sysindex', true), "0");
        app.relaunch(intoIM());
        app.exit();
        return true;
    });

    handle('createInstallLink', (event, args) => {
        requireTrustedRenderer(event);
        const win = getWindow();
        if (process.platform !== 'win32') return dialog.showErrorBox('Unsupported', 'Only supported on Windows.');
        const index = parseInstallationIndex(args?.[0]);
        if (!fs.existsSync(getInstallationProfilePath(index))) return dialog.showErrorBox('Error', 'Installation profile does not exist.');

        const iName = fs.readFileSync(System.getSystemFileOfIndex('_cname', index), 'utf8');
        const shortcutsCreated = createDesktopShortcut({
            windows: { filePath: process.execPath.replace(/\\/g, '\\\\'), name: `Deltamod Community (${iName})`, arguments: `---system_index=${index}` }
        });
        if (shortcutsCreated) dialog.showMessageBox(win, { type: 'info', title: 'Shortcut Created', message: 'Shortcut created on desktop.' });
    });

    handle('openInstallationFolder', (event, args) => {
        requireTrustedRenderer(event);
        const index = parseInstallationIndex(args?.[0]);
        return shell.openPath(getSystemFolderOfIndex('deltaruneInstall', index));
    });

    handle('undertaleModTool:status', () => {
        const executable = configuredUndertaleModToolExecutable();
        const cliExecutable = configuredCommunityCliExecutable();
        return {
            supported: process.platform === 'win32',
            configured: Boolean(executable),
            executableName: executable ? path.basename(executable) : null,
            cliConfigured: Boolean(cliExecutable),
            cliExecutableName: cliExecutable ? path.basename(cliExecutable) : null
        };
    });
    handle('undertaleModTool:choose', async () => {
        const executable = await chooseUndertaleModToolExecutable();
        return {
            configured: Boolean(executable),
            executableName: executable ? path.basename(executable) : null,
            canceled: executable === null
        };
    });
    handle('undertaleModTool:openInstallation', async (event, args) => {
        const index = parseInstallationIndex(args?.[0]);
        if (!fs.existsSync(getInstallationProfilePath(index))) {
            const error = new Error('Installation profile does not exist.');
            error.code = 'INSTALLATION_NOT_FOUND';
            throw error;
        }

        let executable = configuredUndertaleModToolExecutable();
        if (!executable) executable = await chooseUndertaleModToolExecutable();
        if (!executable) return { launched: false, canceled: true };

        let cliExecutable = configuredCommunityCliExecutable();
        if (!cliExecutable) cliExecutable = await chooseCommunityCliExecutable();
        if (!cliExecutable) return { launched: false, canceled: true };

        const gamePath = KeyValue.readKVSOfIndex('gamePath', index);
        const sourceDataFile = UndertaleModTool.resolveGameDataFile(gamePath);
        const installationNamePath = System.getSystemFileOfIndex('_cname', index);
        const installationName = fs.existsSync(installationNamePath)
            ? fs.readFileSync(installationNamePath, 'utf8').trim()
            : `Installation ${Number(index) + 1}`;
        const gameId = KeyValue.readKVSOfIndex('gamePid', index);
        const workspace = await UndertaleModTool.createWorkspace({
            workspaceRoot: path.join(app.getPath('userData'), 'tool-workspaces', 'undertale-mod-tool'),
            sourceDataFile,
            cliExecutable,
            installationIndex: index,
            installationName,
            gameId,
            author: os.userInfo().username
        });
        const result = await UndertaleModTool.launchEditor(executable, workspace.dataFile);
        return {
            launched: result.launched,
            executableName: path.basename(result.executable),
            dataFileName: path.basename(result.dataFile),
            workspacePath: workspace.workspace,
            sourceSha256: workspace.sourceSha256,
            workCopy: true
        };
    });

    // Folders & Misc
    handle('openSysFolder', (event, args) => shell.openPath(args[0] === 'mods' ? getPacketDatabase() : getSystemFolder('deltaruneInstall', false)));
    handle('openModFolder', (event, args) => shell.openPath(Modstore.resolveModFolder(String(args[0]), true)));
    handle('getUniqueFlag', (event, args) => {
        requireTrustedRenderer(event);
        return KeyValue.readUniqueFlag(parseUniqueFlagName(args?.[0]));
    });
    handle('setUniqueFlag', (event, args) => {
        requireTrustedRenderer(event);
        if (typeof args?.[1] !== 'boolean') throw new Error('Preference flags require a boolean value.');
        return KeyValue.writeUniqueFlag(parseUniqueFlagName(args[0]), args[1]);
    });
    handle('fetchSharedVariable', (event, args) => {
        requireTrustedRenderer(event);
        const name = String(args?.[0] ?? '');
        if (!['errorMessage', 'gb1click'].includes(name)) throw new Error('Unknown shared renderer value.');
        return getSharedVar(name);
    });
    handle('isBaked', () => KeyValue.readKVS('baked'));
    handle('npsCallback', () => { if (state.callbackNPS) { state.callbackNPS(...state.callbackNPSPassWith); state.callbackNPS = null; } });
    handle('executeArgumentCmd', () => {}); // No-op
    handle('openFlagDatabase', () => shell.openPath(path.join(app.getPath('userData'), 'deltamod_system-unique', 'flagDB.config')));
    handle('openCommunityMaintainerProfile', event => {
        requireTrustedRenderer(event);
        return shell.openExternal('https://github.com/cmdr-chara');
    });
    handle('deltamoddersDiscord', async event => {
        requireTrustedRenderer(event);
        const response = await axios.get(require('../package.json').discordAPI, {
            timeout: 10_000,
            maxRedirects: 0,
            responseType: 'json'
        });
        const invite = parseExternalHttpsUrl(response.data?.instant_invite, ['discord.com', 'discord.gg']);
        return shell.openExternal(invite);
    });
    handle('browseFile', async (event, args) => {
        requireTrustedRenderer(event);
        const win = getWindow();
        const name = String(args?.[0] ?? 'Files').replace(/[\r\n\0]/g, ' ').slice(0, 80);
        const extension = String(args?.[1] ?? '').toLowerCase();
        if (!/^[a-z0-9]{1,12}$/.test(extension)) throw new Error('Invalid file filter.');
        const pathdial = await dialog.showOpenDialog(win, { properties: ['openFile'], filters: [{ name, extensions: [extension] }] });
        return pathdial.canceled ? null : pathdial.filePaths[0];
    });
    handle('locateDelta', async () => {
        const win = getWindow();
        const pathdial = await dialog.showOpenDialog(win, { properties: ['openDirectory'] });
        return pathdial.canceled ? null : (validateDeltarune(pathdial.filePaths[0]) ? pathdial.filePaths[0] : "Invalid");
    });
    handle('canReportError', () => !isDevToolsEnabled && !state.updateAvailable);
    
    // Updates
    handle('fireUpdate', async () => {
        const win = getWindow();
        try {
            const updateInfo = await Updates.checkUpdates();
            if (updateInfo.update && !state.ignoreUpdate) {
                if (win) win.webContents.send('updateAvailable', updateInfo);
                state.updateAvailable = true;

                updateStackInfo = updateInfo;
                return true;
            }
            return false;
        } catch { return false; }
    });
    handle('start-update', async (event, args) => {
        if (!updateStackInfo) return;
        
        var pwin = createProgressModal();
        try {
            await Updates.downloadUpdate(progress => {
                if (pwin) updateProgressModal(pwin, null, progress.percentage / 100, 'Downloading update');
            });

            if (pwin && !pwin.isDestroyed()) pwin.close();
            Updates.installUpdate();
        } catch (e) {
            if (pwin && !pwin.isDestroyed()) pwin.close();
            dialog.showErrorBox("Update Failed", "The Community update could not be downloaded or verified. You can download the release manually from GitHub.");
            shell.openExternal('https://github.com/cmdr-chara/deltamod/releases');
            state.ignoreUpdate = true;
            page("main");
        }
    });
    handle('ignore-update', () => { state.ignoreUpdate = true; page("main"); });
    handle('initialize', () => {
        const communityData = path.resolve(app.getPath('userData'));
        const expectedData = path.resolve(app.getPath('appData'), 'Deltamod Community');
        if (communityData !== expectedData) {
            throw new Error('Refusing to erase data because the Community profile path is unexpected.');
        }
        for (const entry of fs.readdirSync(communityData)) {
            fs.rmSync(path.join(communityData, entry), { recursive: true, force: true });
        }
        app.quit();
        return true;
    });

    // Debug Modals / Tracers
    handle('modalTest', async () => {
        const win = getWindow();
        const modal = createProgressModal();
        let x = 0.0;
        const interval = setInterval(() => {
            x += 0.1;
            updateProgressModal(modal, win, x, null);
            if (x >= 1.0) {
                clearInterval(interval);
                setTimeout(() => modal.close(), 250);
            }
        }, 250);
    });
    handle('openElectronTracer', () => {
        if (state.elecTracer) return;
        state.elecTracer = new BrowserWindow({
            width: 500,
            height: 300,
            webPreferences: {
                nodeIntegration: false,
                contextIsolation: true,
                sandbox: true,
                partition: PARTITION,
                preload: path.join(__dirname, '..', 'web', 'views', 'electron-tracer', 'preload.js')
            }
        });
        state.elecTracer.setAlwaysOnTop(true);
        state.elecTracer.setMenuBarVisibility(false);
        state.elecTracer.loadURL('deltapack://web/views/electron-tracer/index.html');
    });
    handle('logElectronAPI', (event, args) => { try { if (state.elecTracer) state.elecTracer.webContents.send('log', args[0]); } catch { state.elecTracer = null; } });

    // DeltamodCLI is a separate project. Open its release page instead of
    // downloading and executing a remote command script inside this process.
    handle('installDeltamodCLI', async () => {
        await shell.openExternal('https://github.com/deltamodders/deltamodCLI/releases');
        return true;
    });

    GamePatching.restore(KeyValue.readKVS('gamePath'));
};
