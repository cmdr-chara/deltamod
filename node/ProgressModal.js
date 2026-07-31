const { BrowserWindow } = require("electron");
const { getWindow } = require("./Utils");
const Paths = require("./Paths");
const { PARTITION } = require("./Config");
const { normalizeProgressFraction } = require("./progress/ProgressValue");

const progressModals = new Set();

/**
 * @returns {BrowserWindow}
 */
function createProgressModal() {
    const modal = new BrowserWindow({
        width: 450,
        height: 210,
        resizable: false,
        maximizable: false,
        frame: false,
        minimizable: false,
        closable: true,
        fullscreenable: false,
        modal: true,
        parent: getWindow(),
        webPreferences: {
            devTools: process.env.DELTAMOD_ENV === 'dev',
            nodeIntegration: false,
            contextIsolation: true,
            sandbox: true,
            preload: Paths.file('web', 'dlmodal', 'preload.js'),
            partition: PARTITION
        }
    });

    progressModals.add(modal);
    modal.on('closed', () => {
        progressModals.delete(modal);
    });

    modal.loadURL('deltapack://web/dlmodal/index.html');
    modal.setMenuBarVisibility(false);

    return modal;
}

/**
 * 
 * @param {BrowserWindow} modal 
 * @param {BrowserWindow?} mainWindow
 * @param {number} frac
 * @param {string?} logPrefix
 */
function updateProgressModal(modal, mainWindow, frac, logPrefix) {
    const normalized = normalizeProgressFraction(frac);
    const percent = Math.round(normalized * 100 * 100) / 100;

    if (logPrefix == null) {
        console.log(`Progress: ${percent}%`);
    } else {
        console.log(`${logPrefix} ${percent}%`);
    }

    if (mainWindow != null && !mainWindow.isDestroyed()) {
        mainWindow.setProgressBar(normalized);
    }

    if (!modal.isDestroyed()) modal.webContents.send('progress', percent);
}

function closeAllProgressModals() {
    for (const modal of progressModals) {
        try { modal.close(); } catch {}
    }
    progressModals.clear();
}

module.exports = {
    createProgressModal,
    updateProgressModal,
    closeAllProgressModals
};
