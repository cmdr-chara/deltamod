const { app, BrowserWindow, ipcMain, dialog, shell, Notification, safeStorage, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs');
const os = require('os');
const crypto = require('crypto');
const { spawn } = require('child_process');
const { Worker } = require('worker_threads');
const https = require('https');
const createDesktopShortcut = require('create-desktop-shortcuts');
const axios = require('axios');
const { z } = require('zod');
var elevate = require('windows-elevate');
// Local modules
const KeyValue = require('./KeyValue');
const System = require('./System');
const { getSystemFile, getSystemFolder, getPacketDatabase, getSystemFolderOfIndex } = require('./System');
const Modstore = require('./Modstore');
const CMode = require('./ControllerMode');
const Updates = require('./Updates');
const GameDB = require('./GameDB');
const GamePlatform = require('./GamePlatform');
const { createProgressModal, updateProgressModal } = require('./ProgressModal');
const GamePatching = require('./GamePatching');
const ProfileMigration = require('./ProfileMigration');
const ModSources = require('./ModSources');
const UndertaleModTool = require('./UndertaleModTool');
const {
    NexusOAuthClient,
    parseNexusOAuthClientId,
    parseStoredNexusOAuthTokens
} = require('./NexusSso');
const { downloadToFile } = require('./security/RemoteSecurity');
const { detectImageType } = require('./security/ImageSecurity');
const { resolveWithin } = require('./security/PathSecurity');
const { extractArchiveAtomic } = require('./security/ArchiveSecurity');
const { getCredentialStorageStatus } = require('./security/CredentialStorage');
const { copyDirectoryAtomic } = require('./storage/StagedCopy');
const { readJsonSync, writeJsonAtomicSync, writeFileAtomicSync } = require('./storage/AtomicStore');
const { EasterEggWindowShaker } = require('./EasterEggWindow');
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

function getNexusCredentialPath() {
    return getSystemFile('nexus-oauth-tokens', true);
}

function getLegacyNexusCredentialPath() {
    return getSystemFile('nexus-api-key', true);
}

function getNexusAuthMetadataPath() {
    return getSystemFile('nexus-auth.json', true);
}

function getNexusAuthMethod() {
    const metadata = readJsonSync(getNexusAuthMetadataPath(), {});
    return metadata?.schemaVersion === 2 && metadata?.method === 'oauth-pkce'
        ? 'oauth-pkce'
        : null;
}

function createNexusPersonalKeyDisabledError() {
    const error = new Error('Personal Nexus Mods API keys are disabled. Use Nexus Mods sign-in.');
    error.code = 'NEXUS_PERSONAL_KEY_DISABLED';
    return error;
}

function clearNexusCredentialFiles() {
    try { fs.rmSync(getNexusCredentialPath(), { force: true }); } catch {}
    try { fs.rmSync(getLegacyNexusCredentialPath(), { force: true }); } catch {}
    try { fs.rmSync(getNexusAuthMetadataPath(), { force: true }); } catch {}
}

function readNexusOAuthTokens() {
    const credentialPath = getNexusCredentialPath();
    const legacyCredentialPath = getLegacyNexusCredentialPath();
    if (!fs.existsSync(credentialPath)) {
        // WebSocket SSO API keys and manually entered keys are not OAuth
        // credentials. Remove either legacy format instead of sending it in a
        // Bearer header or attempting an unsafe migration.
        if (fs.existsSync(legacyCredentialPath)) clearNexusCredentialFiles();
        return null;
    }
    if (getNexusAuthMethod() !== 'oauth-pkce') {
        clearNexusCredentialFiles();
        return null;
    }
    if (!safeStorage.isEncryptionAvailable()) {
        const error = new Error('Secure credential storage is unavailable on this system.');
        error.code = 'SECURE_STORAGE_UNAVAILABLE';
        throw error;
    }
    try {
        return parseStoredNexusOAuthTokens(
            safeStorage.decryptString(fs.readFileSync(credentialPath))
        );
    } catch {
        const error = new Error('The saved Nexus Mods authorization could not be decrypted. Remove it and connect again.');
        error.code = 'NEXUS_CREDENTIAL_INVALID';
        throw error;
    }
}

function storeNexusOAuthTokens(tokens) {
    if (!safeStorage.isEncryptionAvailable()) {
        const error = new Error('Secure credential storage is unavailable on this system.');
        error.code = 'SECURE_STORAGE_UNAVAILABLE';
        throw error;
    }
    const normalized = parseStoredNexusOAuthTokens(tokens);
    try {
        writeFileAtomicSync(
            getNexusCredentialPath(),
            safeStorage.encryptString(JSON.stringify(normalized)),
            { backup: false }
        );
        writeJsonAtomicSync(getNexusAuthMetadataPath(), {
            schemaVersion: 2,
            method: 'oauth-pkce',
            updatedAt: new Date().toISOString()
        }, { backup: false });
        try { fs.rmSync(getLegacyNexusCredentialPath(), { force: true }); } catch {}
    } catch (error) {
        clearNexusCredentialFiles();
        throw error;
    }
}

function toJsonSafe(value, depth = 0) {
    if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
    if (typeof value === 'number') return Number.isFinite(value) ? value : null;
    if (depth > 4 || value === undefined || typeof value !== 'object') return undefined;
    if (Array.isArray(value)) {
        return value.slice(0, 64).map(item => toJsonSafe(item, depth + 1));
    }
    const result = {};
    for (const [key, item] of Object.entries(value).slice(0, 64)) {
        const safe = toJsonSafe(item, depth + 1);
        if (safe !== undefined) result[String(key)] = safe;
    }
    return result;
}

function nexusErrorMetadata(error) {
    const metadata = {};
    if (Number.isInteger(error?.status)) metadata.status = error.status;

    if (Object.prototype.hasOwnProperty.call(error || {}, 'retryAfterMs')) {
        if (error.retryAfterMs === null) {
            metadata.retryAfterMs = null;
        } else if (error.retryAfterMs !== undefined && error.retryAfterMs !== '') {
            const retryAfterMs = Number(error.retryAfterMs);
            if (Number.isFinite(retryAfterMs) && retryAfterMs >= 0) {
                metadata.retryAfterMs = Math.round(retryAfterMs);
            }
        }
    }

    if (Object.prototype.hasOwnProperty.call(error || {}, 'retryAt')) {
        if (error.retryAt === null) {
            metadata.retryAt = null;
        } else if (typeof error.retryAt === 'string' && error.retryAt.trim()) {
            metadata.retryAt = error.retryAt;
        } else if (typeof error.retryAt === 'number' && Number.isFinite(error.retryAt)) {
            metadata.retryAt = error.retryAt;
        }
    }

    const quota = toJsonSafe(error?.quota);
    if (quota !== undefined) metadata.quota = quota;
    return metadata;
}

function serializeNexusError(error, fallbackCode, fallbackMessage) {
    return {
        code: String(error?.code || fallbackCode),
        message: String(error?.message || fallbackMessage),
        ...nexusErrorMetadata(error)
    };
}

function isNexusRateLimitedError(error) {
    const code = String(error?.code || '');
    const quotaExhausted = ['daily', 'hourly'].some(period =>
        Number.isFinite(Number(error?.quota?.[period]?.remaining))
        && Number(error.quota[period].remaining) <= 0
    );
    return Number(error?.status) === 429
        || error?.retryAfterMs !== null && error?.retryAfterMs !== undefined
        || error?.retryAt !== null && error?.retryAt !== undefined
        || quotaExhausted
        || [
            'NEXUS_RATE_LIMITED',
            'NEXUS_QUOTA_EXCEEDED',
            'MOD_SOURCE_RATE_LIMITED',
            'MOD_SOURCE_QUOTA_EXCEEDED'
        ].includes(code);
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

function resolveGameInstallation(deltapath, gameOrId, preferredPlatform) {
    if (typeof deltapath !== 'string' || !deltapath.trim() || deltapath === 'INVALID') return null;
    const game = typeof gameOrId === 'string' ? GameDB.getGameById(gameOrId) : gameOrId;
    return GamePlatform.resolveGameInstallation(game, deltapath, { preferredPlatform });
}

function resolveStoredGameInstallation(store) {
    if (!store || typeof store.gamePath !== 'string') return null;
    return resolveGameInstallation(store.gamePath, store.gamePid, store.gamePlatform);
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
        const gameInstallation = resolveStoredGameInstallation(storeData);

        const cnamePath = path.join(installPath, '_cname');
        const issues = [];
        if (!fs.existsSync(storeJSON)) issues.push('Installation data store is missing');
        if (!gameInstallation) issues.push('Game directory, executable, or GameMaker data file is missing');
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
            platform: gameInstallation?.platform || storeData.gamePlatform || null,
            native: gameInstallation?.native || false,
            canOpenInUndertaleModTool: process.platform === 'win32' && Boolean(gameInstallation),
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
    const easterEggWindowShaker = new EasterEggWindowShaker();
    const packageConfig = require('../package.json');
    const configuredNexusOAuthClientId = parseNexusOAuthClientId(
        process.env.DELTAMOD_NEXUS_OAUTH_CLIENT_ID || packageConfig.nexusOAuthClientId
    );
    const nexusOAuth = new NexusOAuthClient({
        clientId: configuredNexusOAuthClientId,
        scope: process.env.DELTAMOD_NEXUS_OAUTH_SCOPE ?? packageConfig.nexusOAuthScope ?? '',
        openExternal: url => shell.openExternal(parseExternalHttpsUrl(url, ['nexusmods.com']))
    });
    let nexusTokenRefresh = null;
    let nexusCredentialRevision = 0;
    let nexusAuthorizationRevision = null;

    function nexusCredentialOperationCancelled() {
        const error = new Error('The Nexus Mods authorization operation was cancelled.');
        error.code = 'NEXUS_SSO_CANCELLED';
        return error;
    }

    function invalidateNexusCredentials() {
        nexusCredentialRevision += 1;
        clearNexusCredentialFiles();
    }

    function cancelNexusAuthorization() {
        const pending = nexusAuthorizationRevision !== null;
        if (pending && nexusAuthorizationRevision === nexusCredentialRevision) {
            nexusCredentialRevision += 1;
        }
        return nexusOAuth.cancel() || pending;
    }

    async function getValidNexusOAuthTokens({ forceRefresh = false } = {}) {
        const tokens = readNexusOAuthTokens();
        if (!tokens) return null;

        const refreshWindowMs = 60 * 1000;
        const now = Date.now();
        if (!forceRefresh && tokens.expiresAt > now + refreshWindowMs) return tokens;
        if (!nexusOAuth.available) {
            if (!forceRefresh && tokens.expiresAt > now + 5000) return tokens;
            const error = new Error('Nexus Mods sign-in cannot be refreshed without the registered OAuth client ID.');
            error.code = 'NEXUS_SSO_NOT_REGISTERED';
            throw error;
        }

        if (!nexusTokenRefresh) {
            const refreshRevision = nexusCredentialRevision;
            nexusTokenRefresh = nexusOAuth.refresh(tokens).then(refreshed => {
                if (refreshRevision !== nexusCredentialRevision) {
                    throw nexusCredentialOperationCancelled();
                }
                storeNexusOAuthTokens(refreshed);
                return refreshed;
            }).catch(error => {
                if (error?.code === 'NEXUS_OAUTH_REAUTH_REQUIRED'
                    && refreshRevision === nexusCredentialRevision) {
                    invalidateNexusCredentials();
                }
                if (!forceRefresh
                    && refreshRevision === nexusCredentialRevision
                    && error?.code !== 'NEXUS_SSO_CANCELLED'
                    && error?.code !== 'NEXUS_OAUTH_REAUTH_REQUIRED'
                    && tokens.expiresAt > Date.now() + 5000) {
                    return tokens;
                }
                throw error;
            }).finally(() => {
                nexusTokenRefresh = null;
            });
        }
        return nexusTokenRefresh;
    }

    async function withNexusAccessToken(operation) {
        const tokens = await getValidNexusOAuthTokens();
        if (!tokens) return operation(null);
        try {
            return await operation(tokens.accessToken);
        } catch (error) {
            if (error?.code !== 'NEXUS_AUTH_FAILED') throw error;
            const refreshed = await getValidNexusOAuthTokens({ forceRefresh: true });
            return operation(refreshed?.accessToken || null);
        }
    }
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
                    if (!getCredentialStorageStatus(safeStorage).available) return null;
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
    handle('shakeCommunityWindowForEasterEgg', (event, args) => {
        const senderWindow = BrowserWindow.fromWebContents(event.sender);
        return easterEggWindowShaker.setPhase(senderWindow, String(args?.[0] || ''));
    });
    handle('quitCommunityForEasterEgg', () => {
        // This deliberately performs no file or profile operations. The
        // Undertale-inspired ending is presentation only.
        easterEggWindowShaker.stop();
        setImmediate(() => app.quit());
        return { closing: true };
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
    handle('setAppIcon', (event, args) => {
        const source = String(args[0] || '');
        if (!source.startsWith('data:image/png;base64,') || source.length > 96 * 1024) {
            throw new Error('Invalid application icon.');
        }
        const icon = nativeImage.createFromDataURL(source);
        const size = icon.getSize();
        if (icon.isEmpty() || size.width < 16 || size.height < 16 || size.width > 512 || size.height > 512) {
            throw new Error('Invalid application icon.');
        }
        getWindow()?.setIcon(icon);
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

    handle('importTheme', async (event, args) => {
        const win = getWindow();
        const request = args?.[0] && typeof args[0] === 'object' ? args[0] : {};
        const requestedName = String(request.name || '').trim().slice(0, 100);
        const requestedDescription = String(request.description || '').trim().slice(0, 500);
        const includeMusic = request.includeMusic === true;
        const requestedColor = String(request.color || '').toUpperCase();
        const requestedSoulColor = String(request.soulColor || '').toUpperCase();
        if (!requestedName) throw new Error('A theme name is required.');
        if (requestedColor && !/^#[0-9A-F]{6}$/.test(requestedColor)) {
            throw new Error('Invalid theme color.');
        }
        if (requestedSoulColor && !/^#[0-9A-F]{6}$/.test(requestedSoulColor)) {
            throw new Error('Invalid SOUL color.');
        }

        const backgroundSelection = await dialog.showOpenDialog(win, {
            title: 'Choose the theme background',
            properties: ['openFile'],
            filters: [{ name: 'Image files', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }]
        });
        const bgPath = backgroundSelection.filePaths[0];
        if (backgroundSelection.canceled || !bgPath) {
            return { created: false, canceled: true, stage: 'background' };
        }

        let musicPath = null;
        if (includeMusic) {
            const musicSelection = await dialog.showOpenDialog(win, {
                title: 'Choose the optional theme music',
                properties: ['openFile'],
                filters: [{ name: 'Song files', extensions: ['mp3', 'ogg'] }]
            });
            musicPath = musicSelection.filePaths[0];
            if (musicSelection.canceled || !musicPath) {
                return { created: false, canceled: true, stage: 'music' };
            }
        }

        const musicExtension = musicPath ? path.extname(musicPath).toLowerCase() : '';
        const imageExtension = path.extname(bgPath).toLowerCase();
        if (musicPath && !['.mp3', '.ogg'].includes(musicExtension)) {
            throw new Error('Unsupported theme audio type.');
        }
        if (!['.png', '.jpg', '.jpeg', '.webp', '.gif'].includes(imageExtension)) {
            throw new Error('Unsupported theme image type.');
        }
        const detectedImage = await detectImageType(bgPath);
        const expectedImage = imageExtension === '.jpg' ? 'jpeg' : imageExtension.slice(1);
        if (detectedImage !== expectedImage) throw new Error('Theme image signature does not match its extension.');

        const randomSeed = Math.random().toString(36).substring(2, 15);
        const themeId = `custom_${randomSeed}`;
        const customThemesDir = path.join(app.getPath('userData'), 'customThemes');
        for (const directory of ['mus', 'img', 'data']) {
            fs.mkdirSync(path.join(customThemesDir, directory), { recursive: true });
        }

        if (musicPath) {
            fs.copyFileSync(musicPath, path.join(customThemesDir, 'mus', `${themeId}${musicExtension}`));
        }
        fs.copyFileSync(bgPath, path.join(customThemesDir, 'img', `${themeId}${imageExtension}`));

        const config = {
            name: requestedName,
            background: `${themeId}${imageExtension}`,
            description: requestedDescription || 'A custom Deltamod Community theme.',
            mainSong: musicPath ? `${themeId}${musicExtension}` : 'ch5.mp3',
            id: themeId,
            musicTrack: musicPath ? 'Custom music' : 'Base Theme music',
            color: requestedColor || await dominantColor(bgPath),
            soulColor: requestedSoulColor || '#FF0000'
        };

        writeJsonAtomicSync(path.join(customThemesDir, 'data', `${themeId}.theme.json`), config);
        return { created: true, themeId };
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
        const credentialStorage = getCredentialStorageStatus(safeStorage);
        if (!credentialStorage.available) {
            dialog.showMessageBoxSync({
                type: 'error',
                title: 'Secure storage unavailable',
                message: `GameBanana login cannot be saved. ${credentialStorage.reason}`,
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

    // Mod source catalogue and credentials
    handle('modSources:getProviders', () => {
        const game = GameDB.getGameById(KeyValue.readKVS('gamePid'));
        return ModSources.getAvailableProviders(game);
    });
    handle('modSources:browse', async (event, args) => {
        try {
            const request = ModSources.BrowseRequest.parse(args?.[0] || {});
            const game = GameDB.getGameById(KeyValue.readKVS('gamePid'));
            if (!game) {
                const error = new Error('No current game installation is selected.');
                error.code = 'GAME_NOT_SELECTED';
                throw error;
            }
            let result;
            if (request.provider === 'moddb') {
                result = await ModSources.browseModDb({
                    slug: game.sources?.moddb?.slug,
                    query: request.query
                });
            } else if (request.provider === 'nexus') {
                result = await ModSources.browseNexus({
                    domain: game.sources?.nexus?.domain,
                    query: request.query,
                    sort: request.sort
                });
            } else {
                const error = new Error('GameBanana continues to use its compatibility catalogue.');
                error.code = 'MOD_SOURCE_LEGACY_PROVIDER';
                throw error;
            }
            return { ok: true, result };
        } catch (error) {
            const serialized = serializeNexusError(
                error,
                'MOD_SOURCE_BROWSE_FAILED',
                'The selected mod catalogue could not be loaded.'
            );
            return {
                ok: false,
                error: serialized
            };
        }
    });
    handle('modSources:nexusStatus', async () => {
        const baseStatus = {
            ssoAvailable: nexusOAuth.available,
            ssoPending: nexusAuthorizationRevision !== null,
            // Kept as a false compatibility field for older renderers. There
            // is no backend path that accepts or uses personal API keys.
            personalKeyFallbackAllowed: false,
            authMethod: fs.existsSync(getNexusCredentialPath()) && getNexusAuthMethod() === 'oauth-pkce'
                ? 'oauth-pkce'
                : null
        };
        let tokens;
        try {
            tokens = readNexusOAuthTokens();
        } catch (error) {
            const serialized = serializeNexusError(
                error,
                'NEXUS_CREDENTIAL_INVALID',
                'The saved Nexus Mods credential could not be read.'
            );
            return {
                ...baseStatus,
                configured: fs.existsSync(getNexusCredentialPath()),
                connected: false,
                error: serialized.message,
                code: serialized.code,
                ...nexusErrorMetadata(error)
            };
        }
        if (!tokens) return { ...baseStatus, configured: false, connected: false };
        try {
            return {
                ...baseStatus,
                configured: true,
                connected: true,
                ...(await withNexusAccessToken(token => ModSources.validateNexusAccessToken(token)))
            };
        } catch (error) {
            const serialized = serializeNexusError(
                error,
                'NEXUS_STATUS_FAILED',
                'Nexus Mods status could not be loaded.'
            );
            return {
                ...baseStatus,
                configured: true,
                connected: false,
                error: serialized.message,
                code: serialized.code,
                ...nexusErrorMetadata(error)
            };
        }
    });
    handle('modSources:setNexusKey', async () => {
        // Keep the channel for older renderers, but never accept or persist a
        // pasted personal API key in any build configuration.
        throw createNexusPersonalKeyDisabledError();
    });
    handle('modSources:startNexusSso', async event => {
        let authorizationRevision = null;
        const cancelOnRendererClose = () => cancelNexusAuthorization();
        event.sender.once('destroyed', cancelOnRendererClose);
        try {
            if (!nexusOAuth.available) {
                const error = new Error('Nexus Mods sign-in is unavailable until Nexus issues the OAuth client ID.');
                error.code = 'NEXUS_SSO_NOT_REGISTERED';
                throw error;
            }
            if (!safeStorage.isEncryptionAvailable()) {
                const error = new Error('Secure credential storage is unavailable. Nexus Mods sign-in cannot be saved.');
                error.code = 'SECURE_STORAGE_UNAVAILABLE';
                throw error;
            }
            if (nexusAuthorizationRevision !== null) {
                const error = new Error('A Nexus Mods sign-in is already waiting for authorization.');
                error.code = 'NEXUS_SSO_ALREADY_PENDING';
                throw error;
            }
            authorizationRevision = ++nexusCredentialRevision;
            nexusAuthorizationRevision = authorizationRevision;
            const tokens = await nexusOAuth.start();
            const status = await ModSources.validateNexusAccessToken(tokens.accessToken);
            if (authorizationRevision !== nexusCredentialRevision) {
                throw nexusCredentialOperationCancelled();
            }
            storeNexusOAuthTokens(tokens);
            return {
                ok: true,
                status: {
                    configured: true,
                    connected: true,
                    authMethod: 'oauth-pkce',
                    ssoAvailable: true,
                    ssoPending: false,
                    personalKeyFallbackAllowed: false,
                    ...status
                }
            };
        } catch (error) {
            const serialized = serializeNexusError(
                error,
                'NEXUS_SSO_FAILED',
                'Nexus Mods sign-in failed.'
            );
            return {
                ok: false,
                error: serialized
            };
        } finally {
            if (authorizationRevision !== null
                && nexusAuthorizationRevision === authorizationRevision) {
                nexusAuthorizationRevision = null;
            }
            event.sender.removeListener('destroyed', cancelOnRendererClose);
        }
    });
    handle('modSources:cancelNexusSso', () => {
        return cancelNexusAuthorization();
    });
    handle('modSources:clearNexusKey', () => {
        cancelNexusAuthorization();
        invalidateNexusCredentials();
        return true;
    });
    handle('modSources:open', async (event, args) => {
        const provider = ModSources.ProviderId.parse(args?.[0]?.provider);
        const url = String(args?.[0]?.url || '');
        const allowedHosts = provider === 'nexus'
            ? ['nexusmods.com']
            : provider === 'moddb'
                ? ['moddb.com']
                : ['gamebanana.com'];
        await shell.openExternal(parseExternalHttpsUrl(url, allowedHosts));
        return true;
    });
    handle('modSources:downloadNexus', async (event, args) => {
        const request = z.object({
            modId: z.union([z.string(), z.number()]),
            operationId: z.string().regex(/^[a-z0-9-]{1,64}$/i),
            sourceUrl: z.string().url().max(1000)
        }).parse(args?.[0] || {});
        const game = GameDB.getGameById(KeyValue.readKVS('gamePid'));
        const domain = game?.sources?.nexus?.domain;
        if (!domain) {
            const error = new Error('Nexus Mods is not mapped for the selected game.');
            error.code = 'MOD_SOURCE_UNAVAILABLE';
            throw error;
        }
        let resolved;
        try {
            resolved = await withNexusAccessToken(accessToken =>
                ModSources.getNexusPrimaryDownload({
                    domain,
                    modId: request.modId,
                    accessToken
                })
            );
        } catch (error) {
            if ([
                'NEXUS_SSO_REQUIRED',
                'NEXUS_AUTH_REQUIRED',
                'NEXUS_AUTH_FAILED',
                'NEXUS_OAUTH_REAUTH_REQUIRED',
                'NEXUS_MANUAL_DOWNLOAD_REQUIRED',
                'NEXUS_PERSONAL_KEY_DISABLED',
                'NEXUS_RATE_LIMITED'
            ].includes(error.code) || isNexusRateLimitedError(error)) {
                const serialized = serializeNexusError(
                    error,
                    'NEXUS_DOWNLOAD_FAILED',
                    'Nexus Mods download is unavailable.'
                );
                return {
                    ok: false,
                    error: serialized
                };
            }
            throw error;
        }
        const sourceUrl = parseExternalHttpsUrl(request.sourceUrl, ['nexusmods.com']);
        try {
            return await Modstore.downloadModFromURL(
                resolved.downloadUrl,
                (progress, downloaded, state = {}) => {
                    event.sender.send('mod-source-progress', {
                        operationId: request.operationId,
                        phase: state.phase || 'download',
                        completed: downloaded,
                        total: state.total
                            || (progress > 0 ? Math.round(downloaded / (progress / 100)) : 0),
                        currentItem: resolved.fileName
                    });
                },
                {
                    provider: 'nexus',
                    id: String(request.modId),
                    fileId: String(resolved.fileId),
                    url: sourceUrl
                },
                null,
                {
                    maximumBytes: resolved.maximumBytes,
                    allowedHosts: ModSources.isNexusDownloadHost
                }
            );
        } catch (error) {
            const serialized = serializeNexusError(
                error,
                'NEXUS_DOWNLOAD_FAILED',
                'The Nexus Mods archive could not be downloaded.'
            );
            event.sender.send('mod-source-progress', {
                operationId: request.operationId,
                phase: 'failed',
                completed: 0,
                total: 0,
                currentItem: resolved.fileName,
                error: serialized.message,
                errorCode: serialized.code,
                ...nexusErrorMetadata(error)
            });
            if (isNexusRateLimitedError(error)) {
                return { ok: false, error: serialized };
            }
            throw error;
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
            if (mod._incompatibleHASH) {
                mod.isIncompatible = true;
                mod.incompatibilityReason ||= 'Mismatching hashes for files: ' + mod._hashDifferentFiles.map(file => '"' + file + '"').join(', ');
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
            return await Modstore.downloadModFromURL(url, (progress, downloaded, state = {}) => {
                event.sender.send('dlmodURL-progress', {
                    progress,
                    downloaded,
                    total: state.total || 0,
                    phase: state.phase || 'download',
                    queryme: requestId,
                    error: false
                });
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
            const resolution = resolveGameInstallation(
                KeyValue.readKVS('gamePath'),
                kvs,
                KeyValue.readKVS('gamePlatform', null)
            );
            return { loaded: Boolean(resolution), path: kvs };
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

        const gameConfig = GameDB.getGameById(KeyValue.readKVS('gamePid'));
        const gameResolution = resolveGameInstallation(
            installPath,
            gameConfig,
            KeyValue.readKVS('gamePlatform', null)
        );
        if (!gameResolution) {
            errorWin('Could not find a supported executable and GameMaker data file to run.');
            if (win) {
                win.show();
                win.webContents.send('audio', true);
            }
            return false;
        }

        if (KeyValue.readKVS('isSteam') && process.platform === 'win32') {
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

        if (isControllerMode) CMode.stop();

        const configuredLauncher = KeyValue.readKVS('linuxLauncher', null);
        const launcher = GamePlatform.createLaunchSpec(gameResolution, configuredLauncher);

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
            cwd: launcher.cwd,
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
            const gameResolution = resolveGameInstallation(
                pathname,
                KeyValue.readKVS('gamePid'),
                KeyValue.readKVS('gamePlatform', null)
            );
            if (!gameResolution) {
                return dialog.showErrorBox('Error', 'The selected game installation is incomplete or unsupported on this platform.');
            }

            if (KeyValue.readUniqueFlag('HASHCHECKS')) {
                const checked = Modstore.modList().modList;
                const incompatible = checked.find(mod => selectedMods.includes(String(mod.uniqueId)) && mod.isIncompatible);
                if (incompatible) {
                    const message = `Refusing to launch incompatible mod "${incompatible.name}": ${incompatible.incompatibilityReason}`;
                    console.error(message);
                    throw new Error(message);
                }
            }

            GamePatching.restore(pathname);

            const patchOptions = { mapPatchTarget: gameResolution.mapPatchTarget };
            const preview = GamePatching.buildPatchPlan(
                pathname,
                getPacketDatabase(),
                selectedMods,
                patchOptions
            );
            patchOptions.approvedPlan = preview;
            GamePatching.assertCsxRuntimeAvailable(preview);
            if (preview.scripts.length > 0) {
                const scriptCount = preview.scripts.reduce((count, group) => count + group.patches.length, 0);
                const choice = dialog.showMessageBoxSync(win, {
                    type: 'warning',
                    title: 'Run mod scripts?',
                    message: `${scriptCount} selected patch script${scriptCount === 1 ? '' : 's'} can run code on this computer.`,
                    detail: 'Only continue if you trust the selected mods and where you downloaded them. Deltamod runs scripts without administrator privileges, but UndertaleModTool scripts are not sandboxed.',
                    buttons: ['Cancel', 'Run scripts'],
                    defaultId: 0,
                    cancelId: 0,
                    noLink: true
                });
                if (choice !== 1) return false;
            }

            let mods = fs.readdirSync(getPacketDatabase()).filter(f => fs.existsSync(path.join(getPacketDatabase(), f, '__deltaID.json'))).map(f => {
                const dataPath = path.join(getPacketDatabase(), f, '__deltaID.json');
                const data = JSON.parse(fs.readFileSync(dataPath, 'utf8'));
                if (selectedMods.includes(String(data.uniqueId))) data.new = false;
                return data;
            });

            const log = await GamePatching.startGamePatch(pathname, getPacketDatabase(), selectedMods, (log) => {
                win?.webContents.send('gplog', {log, percent: -1});
            }, (percent) => {
                win?.webContents.send('gplog', {log: '', percent});
            }, patchOptions).catch(err => {
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

            for (const folder of fs.readdirSync(getPacketDatabase())) {
                const dataPath = path.join(getPacketDatabase(), folder, '__deltaID.json');
                if (!fs.existsSync(dataPath)) continue;
                const data = JSON.parse(fs.readFileSync(dataPath, 'utf8'));
                if (!selectedMods.includes(String(data.uniqueId))) continue;
                data.new = false;
                writeJsonAtomicSync(dataPath, data);
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
                if (!resolveStoredGameInstallation(store)) {
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

        const sourceResolution = resolveGameInstallation(sourcePath, gameInfo);
        if (!sourceResolution) {
            dialog.showErrorBox('Invalid folder', steam ? 'Game missing from Steam library.' : 'Invalid game installation.');
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
                gamePlatform: sourceResolution.platform,
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
        if (!resolveGameInstallation(candidateGamePath, store.gamePid, store.gamePlatform)) {
            issues.push('Game directory, executable, or GameMaker data file is missing');
        }
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
        const sourceResolution = resolveGameInstallation(sourcePath, gameInfo);
        if (!sourceResolution) {
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
                gamePath: destination,
                gamePlatform: sourceResolution.platform
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
    handle('locateDelta', async (event, args) => {
        const win = getWindow();
        const pathdial = await dialog.showOpenDialog(win, { properties: ['openDirectory'] });
        return pathdial.canceled
            ? null
            : (resolveGameInstallation(pathdial.filePaths[0], args?.[0]) ? pathdial.filePaths[0] : "Invalid");
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
