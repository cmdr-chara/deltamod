(() => {
const locateRoot = document.querySelector('.locate-page');
let importPending = false;
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
async function locateDelta() {
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    try {
        var path = await window.deltamodBackend.invoke('locateDelta',[window.gid]);
        if (path != null && path != "Invalid") {
            document.getElementById('dpath').value = path;
        }
        else if (path === "Invalid") {
            htmlAlert("Warning","The selected folder is not a valid installation for " + window.gidName + ".", [{text:"Ok",resolveWith:'ok'}]);
        }
    } catch (error) {
        htmlAlert("Error", "Could not open the folder picker: " + String(error?.message || error), [{text:"Ok",resolveWith:'ok'}]);
    }
}

async function id() {
    console.log(document.getElementById('dpath').value.replaceAll('\\', '/'));
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    if (importPending) return;
    importPending = true; setImportControlsDisabled(true);
    try {
    const result = await window.deltamodBackend.invoke("createNewInstallation", ["", "locate", (window.currentPageStack.pathOV ? window.currentPageStack.pathOV : document.getElementById('dpath').value).replaceAll('\\', '/'), (window.fromIM == undefined ? false : window.fromIM), window.gid, document.getElementById('copyAnyways').checked ? 'copy' : 'ncopy']);
    if (!result) setImportControlsDisabled(false);
    } catch (error) {
        if (locateRoot.isConnected) await htmlAlert('Import failed', String(error?.message || error), [{ text: 'OK' }]);
        setImportControlsDisabled(false);
    } finally { importPending = false; }
}

async function steam() {
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    if (importPending) return;
    importPending = true; setImportControlsDisabled(true);
    try {
        const imported = await window.deltamodBackend.invoke("createNewInstallation", ["steam", "", "", window.fromIM, window.gid, document.getElementById('copyAnyways').checked ? 'copy' : 'ncopy']);
        if (!imported) {
            const installations = await window.deltamodBackend.invoke('getInstallations', []);
            const existing = installations.find(installation => (
                installation.pid === window.gid && installation.steam === true
            ));
            if (existing?.index != null) {
                await window.deltamodBackend.invoke('changeSystemIndex', [String(existing.index)]);
                page(window.fromIM ? 'installmanager' : 'main');
                return;
            }
            htmlAlert("Warning", "Steam could not find a valid installation for " + window.gidName + ".", [{text:"Ok",resolveWith:'ok'}]);
        }
    } catch (error) {
        htmlAlert("Error", "Could not import from Steam: " + String(error?.message || error), [{text:"Ok",resolveWith:'ok'}]);
    } finally { importPending = false; if (locateRoot.isConnected) setImportControlsDisabled(false); }
}

window.currentPageStack.id = id;

window.currentPageStack.back = function() {
    window.deltamodBackend.invoke('changeSystemIndex', ["0"]);
};

window.currentPageStack.locateDelta = locateDelta;

window.currentPageStack.steam = steam;

window.currentPageStack.downloadDelta = async function() {
    if (importPending) return;
    if (window.gid === 'noid') {
        await htmlAlert('Warning', 'Please select a game.', [{ text: 'OK' }]);
        return;
    }
    importPending = true;
    setImportControlsDisabled(true);
    try {
        const path = await window.deltamodBackend.invoke('downloadGame', [window.gid]);
        if (!locateRoot.isConnected || !path) return;
        locateRoot.querySelector('#dpath').value = path;
        locateRoot.querySelector('#copyAnyways').checked = true;
    } catch (error) {
        if (locateRoot.isConnected) await htmlAlert('Unable to download the game', String(error?.message || error), [{ text: 'OK' }]);
    } finally {
        importPending = false;
        if (locateRoot.isConnected) setImportControlsDisabled(false);
    }
};

var currentGameImportOperation = null;
function formatImportBytes(value) {
    if (!Number.isFinite(value)) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let amount = value;
    let unit = 0;
    while (amount >= 1024 && unit < units.length - 1) {
        amount /= 1024;
        unit += 1;
    }
    return `${amount.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function setImportControlsDisabled(disabled) {
    if (!locateRoot.isConnected) return;
    locateRoot.querySelectorAll('button:not(#cancelGameImport), input').forEach(element => {
        element.disabled = disabled;
    });
    if (!disabled) {
        updateImportMethods();
    }
}

let selectedImportFeatures = new Set();

function updateImportMethods() {
    ['steam', 'autodownload'].forEach(feature => {
        const button = document.getElementById('feat_' + feature);
        const available = selectedImportFeatures.has(feature);
        button.disabled = !available;
        button.style.opacity = available ? 1 : 0.4;
    });
}

var removeGameImportListener = window.preloadAPI.onGameImportProgress(progress => {
    currentGameImportOperation = progress.operationId;
    const container = document.getElementById('gameImportProgress');
    if (!container) return;
    container.hidden = false;
    document.getElementById('gameImportPhase').textContent =
        progress.phase === 'commit' ? 'Finalizing import…' : 'Copying game files…';
    document.getElementById('gameImportAmount').textContent =
        `${formatImportBytes(progress.completed)} / ${formatImportBytes(progress.total)}`;
    document.getElementById('gameImportFile').textContent = progress.currentItem || '';
    const bar = document.getElementById('gameImportBar');
    if (progress.total > 0) bar.value = Math.max(0, Math.min(1, progress.completed / progress.total));
    else bar.removeAttribute('value');
    document.getElementById('cancelGameImport').disabled = progress.phase === 'commit';
});
window._onClosePage = window._onClosePage || [];
window._onClosePage.push(removeGameImportListener);

document.getElementById('cancelGameImport').onclick = async () => {
    if (!currentGameImportOperation) return;
    try {
        await window.deltamodBackend.invoke('cancelGameImport', [currentGameImportOperation]);
        if (locateRoot.isConnected) document.getElementById('gameImportPhase').textContent = 'Cancelling…';
    } catch (error) {
        if (locateRoot.isConnected) await htmlAlert('Unable to cancel import', String(error?.message || error), [{ text: 'OK' }]);
    }
};

(async() => {
    var allFeat = ['steam','autodownload'];
    allFeat.forEach(f => {
        document.getElementById('feat_' + f).disabled = true;
        document.getElementById('feat_' + f).style.opacity = 0.4;
    });
    window.gid = "noid";

    const games = await window.deltamodBackend.invoke('getAvailableGames',[]);
    if (!locateRoot.isConnected) return;
    const gOptions = document.querySelector('.gOptions');

    for (const game of games) {
            const option = document.createElement('button');
            option.type = 'button';
            option.classList.add('game-option');
            option.setAttribute('aria-label', game.name);
            option.setAttribute('aria-pressed', 'false');

            const img = document.createElement('img');
            img.id = game.id;
            img.classList.add('gameIco');
            img.alt = '';
            option.addEventListener('click', function() {
                window.gid = game.id;
                window.gidName = game.name;
                document.querySelectorAll('.game-option').forEach(x => {
                    x.classList.remove('selectedGameIco');
                    x.setAttribute('aria-pressed', 'false');
                });

                option.classList.add('selectedGameIco');
                option.setAttribute('aria-pressed', 'true');

                selectedImportFeatures = new Set((game.availableFeatures || []).map(feature => feature.feat));
                updateImportMethods();
            });
            img.src = './gamesIco/' + game.id+'.png';
            option.appendChild(img);
            gOptions.appendChild(option);

            option.title = game.name;
    }
})();
})();
