const { BrowserWindow, safeStorage, session, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const { getSystemFile } = require('./System');
const console = require('./Console');
const { createCommentRequest } = require('./gamebanana/CommentRequest');
const {
    GAMEBANANA_LOGIN_PARTITION,
    clearGameBananaAuthentication
} = require('./gamebanana/LoginSession');
const {
    requireSecureCredentialStorage
} = require('./security/CredentialStorage');
const {
    isApprovedGameBananaUrl,
    isAuthenticatedUiConfig,
    serializeGameBananaCookies
} = require('./gamebanana/LoginValidation');

const GAMEBANANA_ORIGIN = 'https://gamebanana.com';
const GAMEBANANA_LOGIN_URL = `${GAMEBANANA_ORIGIN}/members/account/login`;
const GAMEBANANA_UI_CONFIG_URL = `${GAMEBANANA_ORIGIN}/apiv12/Member/UiConfig?_sUrl=/`;

async function requestGameBananaUiConfig(token) {
    const cookieHeader = String(token || '').trim();
    if (!cookieHeader) {
        const error = new Error('GameBanana did not provide an authenticated session.');
        error.code = 'GAMEBANANA_LOGIN_VALIDATION_FAILED';
        throw error;
    }
    const response = await axios.get(GAMEBANANA_UI_CONFIG_URL, {
        headers: {
            Cookie: cookieHeader,
            'User-Agent': require('electron').app.userAgentFallback,
            TE: 'Trailers'
        },
        maxRedirects: 0,
        timeout: 15000
    });
    if (!isAuthenticatedUiConfig(response.data)) {
        const error = new Error('GameBanana did not confirm the signed-in account.');
        error.code = 'GAMEBANANA_LOGIN_VALIDATION_FAILED';
        throw error;
    }
    return response.data;
}

function obtainLogin() {
    return new Promise(async (resolve, reject) => {
        let settled = false;
        let loginCheckPending = false;
        let loginWindow = new BrowserWindow({
            width: 800,
            height: 600,
            minimizable: false,
            webPreferences: {
                nodeIntegration: false,
                partition: GAMEBANANA_LOGIN_PARTITION,
                contextIsolation: true,
                sandbox: true,
            }
        });
        loginWindow.webContents.setWindowOpenHandler(({ url }) => {
            if (isApprovedGameBananaUrl(url)) shell.openExternal(url);
            return { action: 'deny' };
        });
        loginWindow.webContents.session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
        loginWindow.webContents.on('will-navigate', (event, url) => {
            if (!isApprovedGameBananaUrl(url)) {
                event.preventDefault();
                settled = true;
                reject(new Error('GameBanana login attempted to navigate to an unapproved host.'));
                loginWindow.close();
            }
        });

        if (process.argv.includes('---controller')) {
            loginWindow.setFullScreen(true);
        }
        else {
            loginWindow.center();
            loginWindow.setResizable(false);
            loginWindow.setFullScreenable(false);
        }

        loginWindow.setMenuBarVisibility(false);
    
        // empty cookies before login
        const cookies = loginWindow.webContents.session.cookies;
        const allCookies = await cookies.get({});
        for (const cookie of allCookies) {
            const cookieDomain = cookie.domain?.replace(/^\./, '');
            if (cookieDomain === 'gamebanana.com' || cookieDomain?.endsWith('.gamebanana.com')) {
                await cookies.remove(`http${cookie.secure ? 's' : ''}://${cookie.domain.replace(/^\./, '')}${cookie.path}`, cookie.name);
            }
        }

        loginWindow.loadURL(GAMEBANANA_LOGIN_URL).catch(error => {
            if (settled) return;
            settled = true;
            reject(error);
            loginWindow.close();
        });

        loginWindow.webContents.on('did-navigate', async (_event, url) => {
            if (settled || loginCheckPending || !isApprovedGameBananaUrl(url)) return;
            const parsed = new URL(url);
            if (!parsed.pathname.startsWith('/members/account')) {
                loginCheckPending = true;
                try {
                    // Request only cookies applicable to the primary HTTPS origin instead
                    // of persisting every cookie created by any GameBanana subdomain.
                    const accountCookies = await loginWindow.webContents.session.cookies.get({
                        url: `${GAMEBANANA_ORIGIN}/`
                    });
                    const cookieHeader = serializeGameBananaCookies(accountCookies);
                    const account = await requestGameBananaUiConfig(cookieHeader);
                    console.log(`Validated GameBanana login for user ID ${account._idMemberRow}.`);
                    settled = true;
                    resolve(cookieHeader);
                    loginWindow.close();
                } catch (error) {
                    settled = true;
                    error.code ||= 'GAMEBANANA_LOGIN_VALIDATION_FAILED';
                    reject(error);
                    loginWindow.close();
                } finally {
                    loginCheckPending = false;
                }
            }
        });
        loginWindow.once('closed', () => {
            if (settled) return;
            settled = true;
            reject(new Error('GameBanana login was cancelled.'));
        });
    });
}

function readLoginToken() {
    requireSecureCredentialStorage(safeStorage);
    const file = getSystemFile('bananapwd', true);
    if (!fs.existsSync(file)) return '';
    return safeStorage.decryptString(fs.readFileSync(file));
}

let uiConfCache = null;

async function getGBUIConf() {
    if (uiConfCache) {
        console.log('Using cached GameBanana UI Config for user ID ' + uiConfCache._idMemberRow);
        return uiConfCache;
    }

    try {
        const token = readLoginToken();
        if (!token) return { _idMemberRow: 0 };
        var uiconf = await requestGameBananaUiConfig(token);

        console.log('Fetched GameBanana UI Config for user ID ' + uiconf._idMemberRow);
        uiConfCache = uiconf;

        return uiconf;
    } catch (error) {
        console.log(`GameBanana session is unavailable: ${error.code || error.message || 'unknown error'}`);
        return { _idMemberRow: 0 };
    }
}

function clearCache() {
    uiConfCache = null;
}

async function clearLoginSession() {
    return clearGameBananaAuthentication({
        electronSession: session,
        removeCredential: () => fs.rmSync(getSystemFile('bananapwd', true), { force: true }),
        clearInMemoryCache: clearCache
    });
}

async function leaveComment(id, comment, model) {
    const request = createCommentRequest(id, comment, model);
    const token = readLoginToken();
    if (!token) {
        const error = new Error('Log in to GameBanana before posting a comment.');
        error.code = 'GAMEBANANA_LOGIN_REQUIRED';
        throw error;
    }

    try {
        const response = await axios.post(request.url, request.payload, {
            headers: {
                'Cookie': token,
                'User-Agent': require('electron').app.userAgentFallback,
                'TE': 'Trailers',
                'Content-Type': 'application/json'
            },
        });

        if (response.status >= 200 && response.status < 300) return true;
    } catch (cause) {
        const status = cause?.response?.status;
        const error = new Error(
            status === 401 || status === 403
                ? 'GameBanana rejected the session. Log in again and retry.'
                : 'GameBanana could not post the comment. Please retry.'
        );
        error.code = 'GAMEBANANA_COMMENT_FAILED';
        error.status = status;
        error.cause = cause;
        throw error;
    }

    const error = new Error('GameBanana did not confirm that the comment was posted.');
    error.code = 'GAMEBANANA_COMMENT_FAILED';
    throw error;
}

async function likeMod(model, id) {
    try {
        var token = readLoginToken();
    }
    catch {
        return false;
    }

    var response = await axios.post(`https://gamebanana.com/apiv12/${model}/${id}/Like`, {}, {
        headers: {
            'Cookie': token,
            'User-Agent': require('electron').app.userAgentFallback,
            'TE': 'Trailers',
            'Content-Type': 'application/json'
        },
    }).catch((error) => {
        return error.response;
    });

    return { status: response.status, data: response.data };
}

async function createDeltamodBackup(name) {
    try {
        var token = readLoginToken();
    }
    catch {
        return { success: false, message: "User not logged in" };
    }

    var response = await axios.post(`https://gamebanana.com/apiv12/Collection/Add`, {
        _bIsPrivate: true,
        _sName: name,
        _sPassword: "deltamod"
    }, {
        headers: {
            'Cookie': token,
            'User-Agent': require('electron').app.userAgentFallback,
            'TE': 'Trailers',
            'Content-Type': 'application/json'
        }
    }).catch((error) => {
        return error.response;
    });

    return { id: response.data._idRow, success: response.data._sStatus == 'SUCCESS', error: response.data._sStatus == 'SUCCESS' ? null : response.data };
}

async function addModToBackup(collectionId, itemId, itemType) {
    try {
        var token = readLoginToken();
    }
    catch {
        return { success: false, message: "User not logged in" };
    }

    var response = await axios.post(`https://gamebanana.com/apiv12/${itemType}/${itemId}/AddToCollection`, {
        _idCollectionRow: collectionId
    }, {
        headers: {
            'Cookie': token,
            'User-Agent': require('electron').app.userAgentFallback,
            'TE': 'Trailers',
            'Content-Type': 'application/json'
        }
    }).catch((error) => {
        return error.response;
    });

    return { success: response.data._sStatus == 'SUCCESS', error: response.data._sStatus == 'SUCCESS' ? null : response.data };
}

async function getCollections() {
    try {
        var token = readLoginToken();
    }
    catch {
        return { success: false, message: "You must be logged in to perform this action" };
    }

    var response = await axios.get(`https://gamebanana.com/apiv12/Tool/20575/AccessorCollections`, {
        headers: {
            'Cookie': token,
            'User-Agent': require('electron').app.userAgentFallback,
            'TE': 'Trailers',
            'Content-Type': 'application/json'
        }
    }).catch((error) => {
        return error.response;
    });

    return response.data._aAllCollections || [];
}

async function getCollectionMods(collectionId) {
    try {
        var token = readLoginToken();
    }
    catch {
        return { success: false, message: "User not logged in" };
    }

    var allMods = [];
    var page = 0;
    while (true) {
        page++;
        var response = await axios.get(`https://gamebanana.com/apiv12/Collection/${collectionId}/Items?_nPage=${page}&_sDirection=DESC&_sNameOperator=contains`, {
            headers: {
                'Cookie': token,
                'User-Agent': require('electron').app.userAgentFallback,
                'TE': 'Trailers',
                'Content-Type': 'application/json'
            }
        }).catch((error) => {
            return error.response;
        });

        allMods = allMods.concat(response.data._aRecords || []);

        if (response.data._aMetadata._bIsComplete == true) {
            break;
        }
    }

    console.log(`Found ${allMods.length} mods in collection ${collectionId}`);
    
    var allDownloads = [];
    for (const mod of allMods) {
        var profilepage = await axios.get(`https://gamebanana.com/apiv12/${mod._sModelName}/${mod._idRow}/ProfilePage`);
        var files = profilepage.data._aFiles
        .filter(x => x._aModManagerIntegrations.map(y => y._idToolRow).includes(20575))
        .map((x) => {
            return {
                url: x._sDownloadUrl.replace('https://gamebanana.com/dl/', 'https://gamebanana.com/mmdl/'),
                filename: x._sFile
            };
        });

        allDownloads.push({
            mod: profilepage.data._sName,
            files: files
        });
    }
    return allDownloads;
}

async function deleteCollection(collectionId) {
    try {
        var token = readLoginToken();
    }
    catch {
        return { success: false, message: "User not logged in" };
    }

    var response = await axios.delete(`https://gamebanana.com/apiv12/Collection/${collectionId}`, {
        headers: {
            'Cookie': token,
            'User-Agent': require('electron').app.userAgentFallback,
            'TE': 'Trailers',
            'Content-Type': 'application/json'
        },
        data: {
            _idReasonRow: 1,
            _sNotes: "<p>Deleted by Deltamod on request of user</p>"
        }
    }).catch((error) => {
        return error.response;
    });

    return { success: response.status == 200, error: response.status == 200 ? null : response.data };
}

module.exports = {
    obtainLogin,
    getGBUIConf,
    leaveComment,
    likeMod,
    collections: {
        create: createDeltamodBackup,
        delete: deleteCollection,
        add: addModToBackup,
        list: getCollections,
        inspect: getCollectionMods
    },
    clearCache,
    clearLoginSession,
    isApprovedGameBananaUrl,
    isAuthenticatedUiConfig,
    serializeGameBananaCookies
};
