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

function obtainLogin() {
    return new Promise(async (resolve, reject) => {
        const isApprovedGameBananaUrl = candidate => {
            try {
                const parsed = new URL(candidate);
                return parsed.protocol === 'https:'
                    && (parsed.hostname === 'gamebanana.com' || parsed.hostname.endsWith('.gamebanana.com'));
            } catch {
                return false;
            }
        };
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

        loginWindow.loadURL('https://gamebanana.com/members/account/login');

        loginWindow.webContents.on('did-navigate', async (_event, url) => {
            if (!isApprovedGameBananaUrl(url)) return;
            const parsed = new URL(url);
            if (!parsed.pathname.startsWith('/members/account')) {
                const allCookies = (await loginWindow.webContents.session.cookies.get({})).filter(c => {
                    const cookieDomain = c.domain?.replace(/^\./, '');
                    return cookieDomain === 'gamebanana.com' || cookieDomain?.endsWith('.gamebanana.com');
                });
                console.log('Found ' + allCookies.length + ' GameBanana account cookies after login: ' + allCookies.map(c => c.name).join(', '));
                const cookieHeader = allCookies.map(cookie => `${cookie.name}=${cookie.value}`).join('; ');
                resolve(cookieHeader);
                loginWindow.close();
            }
        });
        loginWindow.once('closed', () => reject(new Error('GameBanana login was cancelled.')));
    });
}

function readLoginToken() {
    if (!safeStorage.isEncryptionAvailable()) return '';
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
        var token = readLoginToken();
    }
    catch {
        var file = "";
        var token = "";
    }
        var uiconf = await axios.get('https://gamebanana.com/apiv12/Member/UiConfig?_sUrl=/', {
            headers: {
                'Cookie': token,
                // get electron user agent
                'User-Agent': require('electron').app.userAgentFallback,
                'TE': 'Trailers'
            }
        });

        console.log('Fetched GameBanana UI Config for user ID ' + uiconf.data._idMemberRow);

        uiConfCache = uiconf.data;

    return uiconf.data;
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
    clearLoginSession
};
