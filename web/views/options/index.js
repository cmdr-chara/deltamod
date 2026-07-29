(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};

function createSettingControlCell() {
    const td = document.createElement('td');
    td.className = 'setting-control-cell center';
    const control = document.createElement('div');
    control.className = 'setting-control';
    td.appendChild(control);
    return { td, control };
}

async function addCheckboxOption(name, description, flagid, requiresRestart = false, changeHandler = (e) => {}) {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');

    const tdLabel = document.createElement('td');
    const span = document.createElement('span');
    span.className = 'setting-title';
    span.innerText = name;
    tdLabel.appendChild(span);

    tdLabel.appendChild(document.createElement('br'));

    const small = document.createElement('small');
    small.className = 'calibri';
    small.innerHTML = description;
    tdLabel.appendChild(small);

    if (requiresRestart) {
        const restartNote = document.createElement('small');
        restartNote.className = 'calibri';
        restartNote.style.marginTop = '7px';
        restartNote.style.display = 'block';
        restartNote.style.color = '#888';
        restartNote.style.fontSize = 'x-small';
        restartNote.innerText = "Requires a Deltamod Community restart to take effect.";
        tdLabel.appendChild(restartNote);
    }

    const { td: tdInput, control } = createSettingControlCell();

    const input = document.createElement('input');
    input.type = 'checkbox';
    input.id = 'FLAG-' + flagid.toUpperCase();
    input.checked = await window.electronAPI.invoke('getUniqueFlag', [flagid]);
    input.addEventListener('change', async (e) => {
        await window.electronAPI.invoke('setUniqueFlag', [flagid, e.target.checked]);
        await changeHandler(e.target.checked);
    });
    control.appendChild(input);

    tr.appendChild(tdLabel);
    tr.appendChild(tdInput);

    table.appendChild(tr);
}

window.electronAPI.invoke('isDevMode', []).then((devmode) => {
    if (devmode) {
        document.getElementById('b_dev').style.display = 'inline-block';
    }
    else {
        const devBtn = document.getElementById('b_dev');
        if (devBtn) devBtn.remove();
    }
});

async function addSelectOption(name, description, options, requiresRestart = false, changeHandler = (val) => {}, defaultValue = '') {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');

    const tdLabel = document.createElement('td');
    const span = document.createElement('span');
    span.className = 'setting-title';
    span.innerText = name;
    tdLabel.appendChild(span);

    tdLabel.appendChild(document.createElement('br'));

    const small = document.createElement('small');
    small.className = 'calibri';
    small.innerHTML = description;
    tdLabel.appendChild(small);

    if (requiresRestart) {
        const restartNote = document.createElement('small');
        restartNote.className = 'calibri';
        restartNote.style.marginTop = '7px';
        restartNote.style.display = 'block';
        restartNote.style.color = '#888';
        restartNote.style.fontSize = 'x-small';
        restartNote.innerText = 'Requires a Deltamod Community restart to take effect.';
        tdLabel.appendChild(restartNote);
    }

    const { td: tdInput, control } = createSettingControlCell();

    const select = document.createElement('select');
    select.id = 'SELECT-' + name.toUpperCase().replace(/[^A-Z0-9]+/g, '-');

    let firstValue = '';
    for (const option of options) {
        const opt = document.createElement('option');
        if (typeof option === 'object' && option !== null) {
            opt.value = option.value ?? option.id ?? option.key ?? '';
            opt.innerText = option.label ?? option.name ?? String(opt.value);
            if (option.selected) select.value = opt.value;
        } else {
            opt.value = String(option);
            opt.innerText = String(option);
        }
        if (firstValue === '') firstValue = opt.value;
        select.appendChild(opt);
    }

    select.value = defaultValue || firstValue;

    select.addEventListener('change', (e) => {
        changeHandler(e.target.value);
    });

    control.appendChild(select);
    tr.appendChild(tdLabel);
    tr.appendChild(tdInput);
    table.appendChild(tr);
}

async function addButton(name, description, click, buttonText, enabled = true, disabledReason = '', colour = '') {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');

    const tdLabel = document.createElement('td');
    const span = document.createElement('span');
    span.className = 'setting-title';
    span.innerText = name;
    if (colour != '') {
        span.style.color = colour;
    }
    tdLabel.appendChild(span);

    tdLabel.appendChild(document.createElement('br'));

    const small = document.createElement('small');
    small.className = 'calibri';
    small.innerText = description;
    tdLabel.appendChild(small);

    const { td: tdInput, control } = createSettingControlCell();

    const button = document.createElement('button');
    button.innerText = buttonText;
    button.addEventListener('click', click);
    control.appendChild(button);
    if (!enabled) {
        button.disabled = true;
        button.style.opacity = 0.5;
        button.style.cursor = 'not-allowed';
        span.style.opacity = 0.5;
        small.style.opacity = 0.5;
        span.style.fontStyle = 'italic';
        small.style.fontStyle = 'italic';
        if (disabledReason != '') {
            small.innerText = '(' + disabledReason + ')';
        }
    }

    tr.appendChild(tdLabel);
    tr.appendChild(tdInput);

    table.appendChild(tr);
    return button;
}

async function addRowHeader(name) {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');
    const td = document.createElement('td');
    td.colSpan = 2;
    td.className = 'rowheader';
    td.innerHTML = "<div style='display:flex; align-items:center; gap:10px;'>" + name + "</div>"; // make it aligned
    tr.appendChild(td);
    table.appendChild(tr);
}

function appendPathValue(element, value) {
    const pathValue = String(value);
    const path = document.createElement('span');
    path.className = 'profile-destination-path';
    path.title = pathValue;

    for (const part of pathValue.split(/([\\/])/)) {
        if (!part) continue;
        path.appendChild(document.createTextNode(part));
        if (part === '\\' || part === '/') {
            path.appendChild(document.createElement('wbr'));
        }
    }

    element.appendChild(path);
}

async function addInfoRow(name, value, description = '', valueKind = 'text') {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');
    const label = document.createElement('td');
    const status = document.createElement('td');
    label.innerText = name;
    if (description) {
        label.appendChild(document.createElement('br'));
        const small = document.createElement('small');
        small.className = 'calibri';
        small.innerText = description;
        label.appendChild(small);
    }
    status.className = 'calibri';
    if (valueKind === 'path') {
        tr.className = 'profile-destination-row';
        appendPathValue(status, value);
    } else {
        status.innerText = value;
    }
    tr.append(label, status);
    table.appendChild(tr);
}

async function addNexusKeyRow() {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');
    tr.className = 'nexus-key-row';

    const label = document.createElement('td');
    const title = document.createElement('span');
    title.className = 'setting-title';
    title.innerText = 'Personal API key';
    const description = document.createElement('small');
    description.className = 'calibri';
    description.innerText = 'Validated by Nexus Mods, then encrypted with the operating system before it is stored.';
    label.append(title, document.createElement('br'), description);

    const { td, control } = createSettingControlCell();
    control.classList.add('nexus-key-control');
    const input = document.createElement('input');
    input.type = 'password';
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.maxLength = 200;
    input.placeholder = 'Paste API key';
    input.setAttribute('aria-label', 'Nexus Mods personal API key');
    const save = document.createElement('button');
    save.innerText = 'Connect';
    save.onclick = async () => {
        const key = input.value.trim();
        if (!key) return;
        save.disabled = true;
        save.innerText = 'Checking…';
        try {
            await window.communityAPI.modSources.setNexusKey(key);
            input.value = '';
            window._pageArguments = { cat: 'nexus' };
            page('options');
        } catch (error) {
            await htmlAlert(
                'Nexus Mods connection failed',
                error?.message || 'The API key could not be validated.',
                [{ text: 'OK', resolveWith: 'ok' }],
                'error'
            );
        } finally {
            save.disabled = false;
            save.innerText = 'Connect';
        }
    };
    control.append(input, save);
    tr.append(label, td);
    table.appendChild(tr);
}

function formatProfileBytes(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024;
        unit += 1;
    }
    return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

async function runOfficialProfileImport(summary) {
    const operationId = crypto.randomUUID();
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');
    const label = document.createElement('td');
    const status = document.createElement('td');
    const progress = document.createElement('progress');
    const current = document.createElement('small');
    const cancel = document.createElement('button');

    label.innerText = summary.previousImport ? 'Importing Deltamod changes' : 'Importing Deltamod data';
    status.className = 'profile-import-progress';
    progress.max = Math.max(summary.totalBytes || 1, 1);
    progress.value = 0;
    current.className = 'calibri';
    current.innerText = 'Preparing a safe copy...';
    cancel.innerText = 'Cancel import';
    cancel.addEventListener('click', async () => {
        cancel.disabled = true;
        current.innerText = 'Cancelling safely...';
        await window.communityAPI.profile.cancel(operationId);
    });
    status.append(progress, current, cancel);
    tr.append(label, status);
    table.appendChild(tr);

    window.currentPageStack.profileImportProgress = info => {
        if (info.operationId !== operationId) return;
        progress.max = Math.max(info.total || summary.totalBytes || 1, 1);
        progress.value = Math.min(info.completed || 0, progress.max);
        const phaseLabel = {
            hash: 'Verifying',
            copy: 'Copying',
            commit: 'Saving'
        }[info.phase] || 'Working';
        current.innerText = `${phaseLabel}: ${info.currentItem || ''}`;
    };

    try {
        const result = await window.communityAPI.profile.import(operationId);
        progress.value = progress.max;
        current.innerText = 'Import complete. The official profile was not changed.';
        cancel.remove();
        const choice = await htmlAlert(
            'Deltamod data imported',
            `Imported ${result.manifest.installations} installation(s), ${result.manifest.mods} mod(s), and ${result.manifest.themes} custom theme(s). Restart Deltamod Community to load them.`,
            [
                { text: 'Restart now', resolveWith: 'restart' },
                { text: 'Restart later', resolveWith: 'later' }
            ]
        );
        if (choice === 'restart') await window.electronAPI.invoke('restartCommunity', []);
    } catch (error) {
        current.innerText = error.code === 'IMPORT_CANCELLED'
            ? 'Import cancelled. Community staging data was removed.'
            : `Import failed: ${error.message || error}`;
        cancel.remove();
    }
}

var tempLock = false;

window.currentPageStack.cat = async function(cat) {
    if (tempLock) return;
    tempLock = true;
    let tbody = document.querySelector('tbody');
    tbody.innerHTML = '';

    document.getElementById('b_gen').classList.remove('selected');
    document.getElementById('b_ui').classList.remove('selected');
    document.getElementById('b_inst').classList.remove('selected');
    document.getElementById('b_data').classList.remove('selected');
    document.getElementById('b_adv').classList.remove('selected');
    document.getElementById('b_gb').classList.remove('selected');
    document.getElementById('b_nexus').classList.remove('selected');
    
    try {
        document.getElementById('b_dev').classList.remove('selected');
    }
    catch (e) {
        console.log('Dev button not found, skipping.');
    }

    if (!document.getElementById('b_' + cat)) {
        cat = 'gen';
    }
    document.getElementById('b_' + cat).classList.add('selected');
    document.querySelectorAll('[id^="b_"]').forEach(btn => {
        if (btn.id != 'b_' + cat) {
            btn.classList.add('blur');
        }
        else {
            btn.classList.remove('blur');
        }
    });
    switch (cat) {
        case 'gen':            
            await addButton("Open mod folder", "Open the folder where your mods are stored.", async () => {
                await window.electronAPI.invoke('openSysFolder', ['mods']);
            }, "Open");
            await addButton("Delete all Community data", "Deletes Community installations, mods, and options. Official Deltamod data is not changed.", async () => {
                page('deleteall');
            }, "Delete", true, '', 'red');
            await addCheckboxOption("Prompt controller mode when available", "When enabled, you will be asked to activate Controller Mode when a compatible controller is attached. Currently only compatible with DualSense.", 'CONTROLLER');
            await addCheckboxOption("Enable hash checks", "Checks mod hashes for compatibility. This may make scans slower.", 'hashchecks', true);
            break;
        case 'ui':
            await addCheckboxOption("Enable music in menus", "Plays background music in the main menus.", 'audio', false, async (enabled) => {
                if (enabled) {
                    var a = new Audio();
                    a.src = 'audio/orch1.mp3';
                    a.playbackRate = 1.3;
                    a.play();
                    currentAudio = "";
                    await page(pageN);
                }
                else {
                    releaseAudioBuffer();
                }
            });
            await addCheckboxOption("Enable SFX in menus", "Plays sound effects in the main menus.", 'sfx', false, (enabled) => {
                if (enabled) {
                    var a = new Audio();
                    a.src = 'audio/orch1.mp3';
                    a.playbackRate = 1.1;
                    a.play();
                }
            });
            await addCheckboxOption("Enable dynamic music", "Enables dynamic background music that changes based on the page. If unchecked, always plays the default music for your theme.", 'dynamusic', true);

            await addSelectOption(
                "Alert alignment",
                "Choose how alerts are positioned on the screen.",
                [
                    { value: "Top", label: "Top" },
                    { value: "Center", label: "Center" },
                    { value: "Bottom", label: "Bottom" },
                    { value: "Separate", label: "Separate" }
                ],
                true,
                async (val) => {
                    var oldVal = localStorage.getItem('alertAlignment');
                    localStorage.setItem('alertAlignment', val);
                    await reapplyHAStyles();
                    var response = await htmlAlert("Modified", "This is how your alerts look when aligned as " + val + '. Keep it this way?', [{ text: "Yes", resolveWith: 'Y' }, { text: "No, revert to " + oldVal, resolveWith: 'N' }]);
                    if (response == 'N') {
                        localStorage.setItem('alertAlignment', oldVal);
                        await reapplyHAStyles();
                        window._pageArguments = { cat: 'ui' };
                        page('options');
                    }
                },
                localStorage.getItem('alertAlignment') || 'Top'
            );

            await addButton("Select a theme", "Opens the theme selection menu.", async () => {
                page('themesel');
            }, "Open");

            break;
        case 'inst':
            var isSteam = await window.electronAPI.invoke('isCurrentIndexSteam', []);

            await addButton("Disconnect Steam", "Stops launching the current Community installation through Steam.", async () => {
                await window.electronAPI.invoke('removeSteamIntegration', []);
            }, "Disconnect", isSteam, "Only available for games imported from Steam.");

            await addButton("Open the Install Manager", "Opens the install manager menu, which allows you to delete/create installations and create shortcuts for them.", async () => {
                page('installmanager');
            }, "Open");

            break;
        case 'data': {
            await addRowHeader(`${icon('database', '20px')} Deltamod compatibility`);
            const summary = await window.communityAPI.profile.summary();
            if (!summary.exists) {
                await addInfoRow(
                    'Official Deltamod profile',
                    'Not found',
                    'Deltamod Community uses separate storage and will not alter official Deltamod data.'
                );
                break;
            }

            await addInfoRow('Detected Deltamod version', summary.version || 'Unknown');
            await addInfoRow('Installations', String(summary.installations || 0));
            await addInfoRow('Installed mods', String(summary.mods || 0));
            await addInfoRow('Custom themes', String(summary.themes || 0));
            await addInfoRow('Required copy space', formatProfileBytes(summary.totalBytes));
            await addInfoRow('Available destination space', formatProfileBytes(summary.availableBytes));
            await addInfoRow('Community destination', summary.destinationRoot, '', 'path');

            if (summary.previousImport) {
                await addInfoRow('Last import', new Date(summary.previousImport.importedAt).toLocaleString());
            }

            await addButton(
                summary.previousImport ? 'Import changes from Deltamod' : 'Import from Deltamod',
                'Creates a validated copy in Community storage. Official Deltamod remains unchanged and usable.',
                () => runOfficialProfileImport(summary),
                summary.previousImport ? 'Import changes' : 'Import data',
                summary.canImport,
                'Not enough free space for a safe copy.'
            );
            break;
        }
        case 'adv':
            await addRowHeader(icon('warning', '20px') + ' ' + "Please only change these settings if you know what they do.");

            await addButton("Reboot in Developer Mode", "Reboots in developer mode, a mode which allows you to use the DevTools.", async () => {
                var goOn = await htmlAlert(
                        'Warning', 
                        "Warning: this is only for users who know what they're doing. Are you sure you want to reboot in developer mode?", 
                        [{text:"Yes",resolveWith:'ok'}, {text:"No",rejectWith:'cancel'}]
                    );
                await window.electronAPI.invoke('rebootDev', [])
            }, "Open", !await window.electronAPI.invoke('isDevMode', []), "You are already in developer mode.");

            let hashButton;
            hashButton = await addButton("Precalculate game hashes", "Builds the Community-owned cache used by advanced mod checks. Game files are not modified.", async () => {
                hashButton.disabled = true;
                hashButton.innerText = 'Scanning…';
                window.currentPageStack.hashProgress = progress => {
                    const completed = Number(progress.completed) || 0;
                    const total = Number(progress.total) || 0;
                    hashButton.innerText = total > 0
                        ? `Hashing ${completed}/${total}`
                        : 'Hashing…';
                };
                try {
                    const result = await window.electronAPI.invoke('precalcGameHashes', []);
                    await htmlAlert("Hash cache ready", `Cached ${result.fileCount} game file(s).`, [{text: "OK", resolveWith:''}]);
                } catch (error) {
                    await htmlAlert("Hashing failed", error?.message || 'The game hash cache could not be built.', [{text: "OK", resolveWith:''}]);
                } finally {
                    delete window.currentPageStack.hashProgress;
                    hashButton.disabled = false;
                    hashButton.innerText = 'Build cache';
                }
            }, "Build cache");

            await addButton(
                "DeltamodCLI releases",
                "Opens the separate DeltamodCLI project. Community does not automatically execute downloaded installer scripts.",
                async () => window.electronAPI.invoke('installDeltamodCLI', []),
                "View releases"
            );

            break;
        // dev isnt keyed and is always in english
        case "dev":
            await addRowHeader(icon('warning', '20px') + ' ' + "These options are for developers only.");
            await addButton('Open flag database (DEV-ONLY)', 'Opens the database holding flags.', async () => {
                await window.electronAPI.invoke('openFlagDatabase', []);
            }, "Open");
            await addButton('Force controller mode (DEV-ONLY)', 'Forces Controller Mode on, regardless of controller detection status', async () => {
                await window.electronAPI.invoke('cmode-on', []);
            }, "Open");
            break;
        case 'gb':
            await invoke('eraseGamebananaCache', []);

            var loadtr = document.createElement('tr');
            loadtr.innerHTML = '<td colspan="2" style="text-align:center; display: flex; justify-content: center; align-items: center;"><div class="loadingBar"></div></td>';
            tbody.appendChild(loadtr);

            var tr = document.createElement('tr');
            tbody.appendChild(tr);

            var td = document.createElement('td');
            td.colSpan = 2;
            td.innerHTML = "Loading...";

            var gamebananaUserinfo = await Promise.race([
                window.electronAPI.invoke('getGamebananaUserinfo', []),
                new Promise(resolve => setTimeout(() => resolve({ loggedIn: false }), 5000))
            ]);
            
            tbody.removeChild(loadtr);
            td.innerHTML = '';

            var flexdiv = document.createElement('div');
            flexdiv.style.display = 'flex';
            flexdiv.style.alignItems = 'center';
            flexdiv.style.gap = '10px';
            td.appendChild(flexdiv);

            var img = document.createElement('img');
            img.src = gamebananaUserinfo._sAvatarUrl || './img/mod-placeholder.png';
            img.style.width = '32px';
            img.style.height = '32px';
            img.style.border = '1px solid var(--theme-color)';
            flexdiv.appendChild(img);

            img.style.borderRadius = '5px';
            var span = document.createElement('span');
            flexdiv.appendChild(span);
            span.innerText = `Currently logged in as ${gamebananaUserinfo._sName}`;

            if (gamebananaUserinfo._sName == undefined) {
                span.innerText = "You aren't logged in to GameBanana.";
                gamebananaUserinfo = { loggedIn: false };
            }
            else {
                gamebananaUserinfo.loggedIn = true;
            }

            tr.appendChild(td);

            if (gamebananaUserinfo.loggedIn && gamebananaUserinfo._sName != undefined) {
                await addButton("Logout", "Removes your GameBanana account from Deltamod.", async () => {
                    await window.electronAPI.invoke('logoutGamebanana', []);
                    window._pageArguments = {cat: 'gb'};
                    page('options');
                }, "Logout", gamebananaUserinfo.loggedIn, "You aren't logged in to GameBanana.", '');
            }
            else {
                await addButton("Login", "Adds a GameBanana account to Deltamod.", async () => {
                    try {
                        const loggedIn = await window.electronAPI.invoke('loginGamebanana', []);
                        if (loggedIn) {
                            window._pageArguments = {cat: 'gb'};
                            page('options');
                        }
                    } catch (error) {
                        await htmlAlert(
                            'GameBanana login failed',
                            error?.message || 'GameBanana could not verify the signed-in account.',
                            [{ text: 'OK', resolveWith: 'ok' }],
                            'error'
                        );
                    }
                }, "Login", !gamebananaUserinfo.loggedIn, "You are already logged in to GameBanana.", '');
            }
            break;
        case 'nexus': {
            await addRowHeader(`${icon('key', '20px')} Nexus Mods`);
            const status = await window.communityAPI.modSources.nexusStatus();
            if (status.connected) {
                await addInfoRow('Connection', `Connected as ${status.name}`);
                await addInfoRow(
                    'Authentication',
                    status.authMethod === 'sso' ? 'Nexus Mods single sign-on' : 'Personal API key (beta fallback)'
                );
                await addInfoRow(
                    'Download access',
                    status.premium ? 'Premium API downloads available' : 'Website confirmation may be required',
                    status.premium
                        ? 'Compatible archives can be downloaded and imported directly.'
                        : 'Nexus Mods restricts direct API downloads for non-premium accounts.'
                );
                await addButton(
                    'Disconnect Nexus Mods',
                    'Removes the encrypted Nexus Mods credential from this device.',
                    async () => {
                        await window.communityAPI.modSources.clearNexusKey();
                        window._pageArguments = { cat: 'nexus' };
                        page('options');
                    },
                    'Disconnect'
                );
            } else {
                await addInfoRow(
                    'Connection',
                    status.configured ? 'Saved key needs attention' : 'Not connected',
                    status.error || 'Connect a Nexus Mods account to browse its catalogue.'
                );
                if (status.ssoAvailable) {
                    let signingIn = false;
                    let ssoButton;
                    const toggleSso = async () => {
                        if (signingIn) {
                            await window.communityAPI.modSources.cancelNexusSso();
                            return;
                        }
                        signingIn = true;
                        ssoButton.innerText = 'Cancel sign-in';
                        try {
                            await window.communityAPI.modSources.startNexusSso();
                            window._pageArguments = { cat: 'nexus' };
                            page('options');
                        } catch (error) {
                            if (error?.code !== 'NEXUS_SSO_CANCELLED') {
                                await htmlAlert(
                                    'Nexus Mods sign-in failed',
                                    error?.message || 'The Nexus Mods account could not be connected.',
                                    [{ text: 'OK', resolveWith: 'ok' }],
                                    'error'
                                );
                            }
                        } finally {
                            signingIn = false;
                            ssoButton.innerText = 'Sign in';
                        }
                    };
                    ssoButton = await addButton(
                        'Sign in with Nexus Mods',
                        'Opens Nexus Mods in your browser. Authorization returns directly to Community; no API key needs to be copied.',
                        toggleSso,
                        status.ssoPending ? 'Cancel sign-in' : 'Sign in'
                    );
                    signingIn = Boolean(status.ssoPending);
                } else {
                    await addInfoRow(
                        'Single sign-on',
                        'Awaiting Nexus registration',
                        'The integration is implemented, but Nexus Mods must issue the application slug before this button can be enabled.'
                    );
                }
                if (status.personalKeyFallbackAllowed) {
                    await addInfoRow(
                        'Beta fallback',
                        'Personal API key',
                        'Available only in testing builds or while public SSO registration is pending.'
                    );
                    await addNexusKeyRow();
                }
            }
            await addButton(
                'Nexus Mods API access',
                'Opens the official Nexus Mods page where personal API keys are managed.',
                () => window.communityAPI.modSources.open({
                    provider: 'nexus',
                    url: 'https://www.nexusmods.com/users/myaccount?tab=api%20access'
                }),
                'Open key page'
            );
            break;
        }
    }
    // theme adjustments
    // as far as i know this page is the only page that needs ts
    genbtnstyles();
    rew();

    tempLock = false;
}

if (window._pageArguments?.cat != undefined && typeof window.currentPageStack?.cat === 'function') {
    const selectedCategory = window._pageArguments.cat;
    window._pageArguments = {};
    window.currentPageStack.cat(selectedCategory);
}
})();
