(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
async function locateDelta() {
    var path = await window.electronAPI.invoke('locateDelta',[]);
    if (path != null && path != "Invalid") {
        document.querySelector('input[type="text"]').value = path;
    }
    else {
        htmlAlert("Warning","The selected folder is not a valid installation for " + window.gidName + ".", [{text:"Ok",resolveWith:'ok'}]);
    }
}

async function id() {
    console.log(document.getElementById('dpath').value.replaceAll('\\', '/'));
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    setImportControlsDisabled(true);
    const result = await window.electronAPI.invoke("createNewInstallation", ["", "locate", (window.currentPageStack.pathOV ? window.currentPageStack.pathOV : document.getElementById('dpath').value).replaceAll('\\', '/'), (window.fromIM == undefined ? false : window.fromIM), window.gid, document.getElementById('copyAnyways').checked ? 'copy' : 'ncopy']);
    if (!result) setImportControlsDisabled(false);
}

async function steam() {
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    await window.electronAPI.invoke("createNewInstallation", ["steam", "", "", window.fromIM, window.gid, document.getElementById('copyAnyways').checked ? 'copy' : 'ncopy']);
}

window.currentPageStack.id = id;

window.currentPageStack.back = function() {
    window.electronAPI.invoke('changeSystemIndex', ["0"]);
};

window.currentPageStack.locateDelta = locateDelta;

window.currentPageStack.steam = steam;

window.currentPageStack.downloadDelta = async function() {
    if (window.gid == 'noid') {
        htmlAlert("Warning","Please select a game.",[{text:"Ok",resolveWith:'ok'}]);
        return;
    }
    var path = await window.electronAPI.invoke("downloadGame", [window.gid]);
    if (path) {
        document.querySelector('input[type="text"]').value = path;
    }

    document.querySelector('.copyAnyways').style.opacity = 0.5;
    document.querySelector('.copyAnyways').style.pointerEvents = 'none';
    document.querySelector('#copyAnyways').checked = true;
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
    document.querySelectorAll('button:not(#cancelGameImport), input').forEach(element => {
        element.disabled = disabled;
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
    document.getElementById('gameImportBar').value =
        progress.total > 0 ? Math.min(1, progress.completed / progress.total) : 0;
});
window._onClosePage = window._onClosePage || [];
window._onClosePage.push(removeGameImportListener);

document.getElementById('cancelGameImport').onclick = async () => {
    if (!currentGameImportOperation) return;
    await window.electronAPI.invoke('cancelGameImport', [currentGameImportOperation]);
    document.getElementById('gameImportPhase').textContent = 'Cancelling…';
};

(async() => {
    var allFeat = ['steam','autodownload'];
    allFeat.forEach(f => {
        document.getElementById('feat_' + f).disabled = true;
        document.getElementById('feat_' + f).style.opacity = 0.4;
    });
    window.gid = "noid";

    var games = await window.electronAPI.invoke('getAvailableGames',[]);
    var gOptions = document.querySelector('.gOptions');

    var ems = [];

    for (l in games) {
        await (async() => {
            var game = games[l];

            var img = document.createElement('img');
            img.id = game.id;
            img.classList.add('gameIco');
            img.addEventListener('click', function() {
                window.gid = game.id;
                window.gidName = game.name;
                document.querySelectorAll('.gameIco').forEach(x =>{
                    x.classList.remove('selectedGameIco');
                });

                img.classList.add('selectedGameIco');

                var allFeat = ['steam','autodownload'];
                allFeat.forEach(f => {
                    if (game.availableFeatures.map(x => x.feat).includes(f)) {
                        document.getElementById('feat_' + f).disabled = false;
                        document.getElementById('feat_' + f).style.opacity = 1;
                    }
                    else {
                        document.getElementById('feat_' + f).disabled = true;
                        document.getElementById('feat_' + f).style.opacity = 0.4;
                    }
                });
            })
            img.src = './gamesIco/' + game.id+'.png';
            gOptions.appendChild(img);

            ems.push({id:game.id,em:img});

            tippy(img, {
                content: game.name
            });
        })();
    }
})();
})();
