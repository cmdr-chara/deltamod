const { app, BrowserWindow, dialog, protocol, session, shell, screen, Menu } = require('electron');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { getConfig, config } = require('7zip-min');
const { path7za } = require('7zip-bin');

const PRODUCT_NAME = 'Deltamod Community';
const PROTOCOL_SCHEME = 'deltamod-community';
app.setName(PRODUCT_NAME);
if (process.env.DELTAMOD_TEST === '1' && process.env.DELTAMOD_TEST_USER_DATA) {
    const isolatedAppData = path.join(path.resolve(process.env.DELTAMOD_TEST_USER_DATA), 'appData');
    fs.mkdirSync(isolatedAppData, { recursive: true });
    app.setPath('appData', isolatedAppData);
}
const communityDataPath = process.env.DELTAMOD_TEST === '1' && process.env.DELTAMOD_TEST_USER_DATA
    ? path.resolve(process.env.DELTAMOD_TEST_USER_DATA)
    : path.join(app.getPath('appData'), PRODUCT_NAME);
fs.mkdirSync(communityDataPath, { recursive: true });
app.setPath('userData', communityDataPath);

// Local modules
const Paths = require('./Paths');
const KeyValue = require('./KeyValue');
const { getSystemFile, setSystemIndex } = require('./System');
const System = require('./System');
const { setWindow, page, between } = require('./Utils');
const CMode = require('./ControllerMode');
const GamePatching = require('./GamePatching');
const ProfileMigration = require('./ProfileMigration');
const Netlayer = require('./Netlayer');
const { writeFileAtomicSync } = require('./storage/AtomicStore');
const console = require('./Console');
const { handleProtocolLaunch, registerProtocolSchemesAsPrivileged, registerProtocolHandlers } = require('./Protocol');
const { isFeatureEnabled } = require('./FeatureFlags');
const { PARTITION } = require('./Config');
const registerIPCHandlers = require('./IPCHandlers');
const { registerWindowZoomShortcuts } = require('./WindowZoom');

// --- Global Setup & State ---
let win;

const isControllerMode = process.argv.includes('-controller');
const isDevToolsEnabled = process.argv.includes('--developer') || process.env.DELTAMOD_ENV === 'dev';

// Shared state object specifically for IPC injection context tracking
const appState = {
    updateAvailable: false,
    ignoreUpdate: false,
    callbackNPS: null,
    callbackNPSPassWith: null,
    elecTracer: null,
    STEAM_BASE: null
};

// --- Initialization ---
app.commandLine.appendSwitch('disable-features', 'MediaSessionService');
registerProtocolSchemesAsPrivileged(protocol);

if (process.argv.includes('--developer') && !isFeatureEnabled("AutoupdateNoMatterWhat")) {
    appState.ignoreUpdate = true;
}

if (process.defaultApp) {
    if (process.argv.length >= 2) {
        app.setAsDefaultProtocolClient(PROTOCOL_SCHEME, process.execPath, [path.resolve(process.argv[1])]);
    }
} else {
    app.setAsDefaultProtocolClient(PROTOCOL_SCHEME);
}

// --- Utilities ---

/**
 * Triggers the fallback error window when a critical failure occurs.
 * @param {Error|string} error - The error to display.
 */
function errorWin(error) {
    if (win) win.setFullScreen(false);
    return require('./ErrorWin.js').errorWin(error);
}

/**
 * Helper to direct the main window to a specific URL.
 * @param {string} url - The URL to load.
 */
function loadUrl(url) {
    win.loadURL(url);
}

/**
 * Generates a SHA-256 hash for a given string.
 * @param {string} str - Input string.
 * @returns {string} The computed hex hash.
 */
function hashString(str) {
    return crypto.createHash('sha256').update(str).digest('hex');
}

/**
 * Utility to pause execution for a set duration using async/await.
 * @param {number} amount - Milliseconds to wait.
 * @returns {Promise<void>}
 */
function asyncTimeout(amount) {
    return new Promise(resolve => setTimeout(resolve, amount));
}

/**
 * Clears the standard console and prints the ASCII logo and current version.
 */
function writeTopPart() {
    process.stdout.write(`\x1b]0;${PRODUCT_NAME}\x07`);
    console.clear();
    process.stdout.write(`${fs.readFileSync(path.join(__dirname, '..', 'ascii.txt'), 'utf8')}\r\n\r\n`);
    process.stdout.write(`[ version ${app.getVersion()} ]\r\n\r\n`);
}

/**
 * Checks if a specific path is a child (subpath) of a parent directory.
 * @param {string} parent - The parent path.
 * @param {string} child - The subpath to test.
 * @returns {boolean}
 */
function isSubpath(parent, child) {
    const a = path.resolve(parent).toLowerCase();
    const b = path.resolve(child).toLowerCase();
    return b.startsWith(a + path.sep) || a === b;
}

/**
 * Moves/copies all files from a wrapper directory into a destination, then deletes the wrapper.
 * @param {string} dest - Destination folder.
 * @param {string} wrapper - Source folder to flatten.
 */
function flattenInto(dest, wrapper) {
    const destR = path.resolve(dest);
    const wrapR = path.resolve(wrapper);
    if (destR === wrapR) return;
    if (!isSubpath(destR, wrapR)) {
        console.warn('[flattenInto] refused: wrapper not inside dest', { destR, wrapR });
        return;
    }

    for (const name of fs.readdirSync(wrapR)) {
        const from = path.join(wrapR, name);
        const to = path.join(destR, name);
        try { fs.rmSync(to, { recursive: true, force: true }); } catch {}
        try {
            fs.renameSync(from, to);
        } catch {
            if (fs.statSync(from).isDirectory()) {
                console.error('Cannot flatten directories recursively in this context.');
            } else {
                fs.mkdirSync(path.dirname(to), { recursive: true });
                fs.copyFileSync(from, to);
            }
            fs.rmSync(from, { recursive: true, force: true });
        }
    }
    try { fs.rmSync(wrapR, { recursive: true, force: true }); } catch {}
}

// --- Window Creation ---

/**
 * Bootstraps the application layout, enforces hardware requirements, configures primary partitioning routes, and constructs the primary BrowserWindow interface instance.
 */
function createWindow() {
    writeTopPart();

    KeyValue.loadUniqueDefaults();
    KeyValue.upgradeStores();
    config({ ...getConfig(), binaryPath: path7za });
    try { System.clearTemporary(); } catch (e) { console.error(e); }

    const sysArg = process.argv.find(a => a.startsWith('---system_index='));
    if (sysArg) {
        try {
            const val = sysArg.split('=')[1];
            if (/^\d+$/.test(val)) writeFileAtomicSync(getSystemFile('_sysindex', true), val);
        } catch {}
    }

    const partOverride = getSystemFile('_sysindex', true);
    if (fs.existsSync(partOverride)) {
        let overrideData = fs.readFileSync(partOverride, 'utf8');
        if (!/^\d+$/.test(overrideData) || !fs.existsSync(path.join(app.getPath('userData'), 'deltamod_system-' + overrideData))) {
            console.error('The specified installation (' + overrideData + ') is invalid.');
            overrideData = '0';
            writeFileAtomicSync(partOverride, overrideData);
        }
        setSystemIndex(overrideData);
    } else {
        setSystemIndex('0');
    }

    registerProtocolHandlers(session.fromPartition(PARTITION));

    const unmetConditions = process.env.DELTAMOD_TEST === '1'
        ? []
        : require('./RunConditions.js').checkConditions();
    if (unmetConditions.length > 0) {
        const requiredUnmet = unmetConditions.filter(c => c.required);
        if (requiredUnmet.length > 0) {
            dialog.showMessageBoxSync({ type: 'error', title: 'PC Requirements Not Met', message: `Missing requirements:\n${requiredUnmet.map(n => n.name).join('\n')}\n\nDeltamod Community will not run.` });
            return app.exit(1);
        } else {
            dialog.showMessageBoxSync({ type: 'warning', title: 'PC Requirements Not Met', message: `Missing suggested requirements:\n${unmetConditions.map(n => n.name).join('\n')}\n\nYou might experience issues.` });
        }
    }

    const bounds = screen.getPrimaryDisplay().workAreaSize;
    
    KeyValue.retrieve();
    win = new BrowserWindow({
        width: 900,
        height: 600,
        resizable: true,
        frame: false,
        fullscreen: isControllerMode,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            sandbox: true,
            spellcheck: false,
            safeDialogs: true,
            partition: PARTITION,
            preload: Paths.file('web', 'preload.js')
        }
    });

    setWindow(win);
    registerWindowZoomShortcuts(win);

    // --- Inject State and Register IPC Handlers ---
    registerIPCHandlers({
        getWindow: () => win,
        isControllerMode,
        isDevToolsEnabled,
        errorWin,
        state: appState
    });

    if (isControllerMode) {
        CMode.start();
        win.setMenu(Menu.buildFromTemplate([
            { label: 'View', submenu: [
                { label: 'Exit Controller Mode', accelerator: 'F11', click: () => win.webContents.send('leave-controller-mode') },
                { label: 'Toggle Developer Tools', accelerator: 'F12', click: () => { if (isDevToolsEnabled) win.webContents.toggleDevTools(); } }
            ]}
        ]));
        win.on('blur', () => CMode.stop());
        win.on('focus', () => CMode.start());
    }

    win.webContents.session.webRequest.onBeforeRequest((details, callback) => {
        if (details.url.startsWith('https://')) {
            const locked = !Netlayer.approve(between(details.url, 'https://', '/'));
            if (locked) errorWin(`A request to an unapproved URL was blocked: ${details.url}`);
            return callback({ cancel: locked });
        }
        callback({ cancel: false });
    });

    win.on('resized', () => {
        let [w, h] = win.getSize();
        if (w < 900) w = 900;
        if (h < 600) h = 600;
        win.setSize(w, h);
        win.webContents.send('winResAlert', []);
    });

    if (!isDevToolsEnabled) win.setMenu(null);
    win.webContents.on('devtools-opened', () => { if (!isDevToolsEnabled) win.webContents.closeDevTools(); });
    win.webContents.on('will-navigate', (event, url) => { if (/^https?:\/\//.test(url)) { event.preventDefault(); shell.openExternal(url); } });
    win.webContents.setWindowOpenHandler(({ url }) => {
        if (/^https:\/\//.test(url)) shell.openExternal(url);
        return { action: 'deny' };
    });
    win.webContents.session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));

    win.loadURL('deltapack://web/index.html');
}

// --- App Lifecycle ---
if (!app.requestSingleInstanceLock()) {
    app.quit();
} else {
    app.on('second-instance', (e, argv) => {
        const maybeUrl = argv.find(arg => arg.startsWith(`${PROTOCOL_SCHEME}://`));
        if (maybeUrl) {
            handleProtocolLaunch(maybeUrl);
            page('goc-dl');
            if (win) win.focus();
        }
    });
}

app.whenReady().then(async () => {
    if (['win32', 'linux'].includes(process.platform)) {
        const maybeUrl = process.argv.find(arg => arg.startsWith(`${PROTOCOL_SCHEME}://`));
        if (maybeUrl) handleProtocolLaunch(maybeUrl);
    }

    try {
        const p = KeyValue.readKVS('deltarunePath');
        if (p) GamePatching.restoreOriginalsIfAny(p);
    } catch {}
    try {
        await ProfileMigration.recoverInterruptedImports(app.getPath('userData'));
    } catch (error) {
        console.error(`Could not recover an interrupted profile import: ${error.message}`);
    }

    createWindow();
});

app.on('window-all-closed', () => {
    try { CMode.stop(); } catch {}
    try { GamePatching.stopOwnedPatchers(); } catch {}
    app.quit();
});

app.on('before-quit', () => {
    try { GamePatching.stopOwnedPatchers(); } catch {}
});

app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
});

module.exports = { loadUrl };
