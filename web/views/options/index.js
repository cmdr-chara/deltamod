(() => {
const localize = (key, fallback, ...args) => (
    window.Localization?.t(key, fallback, ...args) || fallback
);
const localizeKnown = value => (
    window.Localization?.translateKnownText(value) || value
);
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
    name = localizeKnown(name);
    description = localizeKnown(description);
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
    input.setAttribute('aria-label', name);
    input.checked = await window.deltamodBackend.invoke('getUniqueFlag', [flagid]);
    let savedChecked = input.checked;
    input.addEventListener('change', async (e) => {
        const nextChecked = input.checked;
        const saved = await window.FrontendRefinements.saveControl(input, () =>
            window.deltamodBackend.invoke('setUniqueFlag', [flagid, nextChecked]),
            () => { input.checked = savedChecked; });
        if (saved) {
            savedChecked = nextChecked;
            try { await changeHandler(nextChecked); }
            catch (error) { await htmlAlert(name, String(error?.message || error), [{ text: 'OK', resolveWith: 'ok' }]); }
        }
    });
    control.appendChild(input);

    tr.appendChild(tdLabel);
    tr.appendChild(tdInput);

    table.appendChild(tr);
}

function addRangeOption(name, description, {
    min = 0,
    max = 100,
    step = 1,
    value = 50,
    unit = '%',
    changeHandler = () => {}
} = {}) {
    name = localizeKnown(name);
    description = localizeKnown(description);
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');

    const tdLabel = document.createElement('td');
    const title = document.createElement('span');
    title.className = 'setting-title';
    title.innerText = name;
    tdLabel.appendChild(title);
    tdLabel.appendChild(document.createElement('br'));

    const help = document.createElement('small');
    help.className = 'calibri';
    help.innerText = description;
    tdLabel.appendChild(help);

    const { td: tdInput, control } = createSettingControlCell();
    tdInput.classList.add('setting-range-cell');
    control.classList.add('setting-range-control');

    const input = document.createElement('input');
    input.type = 'range';
    input.min = String(min);
    input.max = String(max);
    input.step = String(step);
    input.value = String(value);
    input.className = 'setting-range';
    input.id = 'MUSIC-VOLUME';
    input.setAttribute('aria-label', name);

    const output = document.createElement('output');
    output.className = 'setting-range-value';
    output.htmlFor = input.id;

    const updateValue = () => {
        const numericValue = Number(input.value);
        output.value = `${numericValue}${unit}`;
        output.textContent = output.value;
        changeHandler(numericValue);
    };
    input.addEventListener('input', updateValue);
    output.value = `${input.value}${unit}`;
    output.textContent = output.value;

    control.append(input, output);
    tr.append(tdLabel, tdInput);
    table.appendChild(tr);
    return input;
}

window.deltamodBackend.invoke('isDevMode', []).then((devmode) => {
    const devBtn = document.getElementById('b_dev');
    if (!devBtn) return;

    if (devmode) {
        // Let the category stylesheet control the button layout (flex, icon gap,
        // focus treatment) instead of overriding it with an inline display mode.
        devBtn.style.removeProperty('display');
    } else {
        devBtn.remove();
    }
});

async function addSelectOption(name, description, options, requiresRestart = false, changeHandler = (val) => {}, defaultValue = '') {
    name = localizeKnown(name);
    description = localizeKnown(description);
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
            opt.innerText = localizeKnown(option.label ?? option.name ?? String(opt.value));
            if (option.selected) select.value = opt.value;
        } else {
            opt.value = String(option);
            opt.innerText = localizeKnown(String(option));
        }
        if (firstValue === '') firstValue = opt.value;
        select.appendChild(opt);
    }

    select.value = defaultValue || firstValue;
    select.setAttribute('aria-label', name);
    let savedValue = select.value;
    select.addEventListener('change', async () => {
        const nextValue = select.value;
        const saved = await window.FrontendRefinements.saveControl(select, () => changeHandler(nextValue),
            () => { select.value = savedValue; });
        if (saved) savedValue = nextValue;
    });

    control.appendChild(select);
    tr.appendChild(tdLabel);
    tr.appendChild(tdInput);
    table.appendChild(tr);
    return select;
}

async function addButton(name, description, click, buttonText, enabled = true, disabledReason = '', colour = '') {
    name = localizeKnown(name);
    description = localizeKnown(description);
    buttonText = localizeKnown(buttonText);
    disabledReason = localizeKnown(disabledReason);

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
        tr.classList.add('setting-row-disabled');
        button.disabled = true;
        if (disabledReason != '') {
            small.innerText = '(' + disabledReason + ')';
            button.title = disabledReason;
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
    name = localizeKnown(name);
    value = valueKind === 'path' ? value : localizeKnown(value);
    description = localizeKnown(description);

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

async function addLanguageOption(language, selected) {
    const table = document.querySelector('tbody');
    const tr = document.createElement('tr');
    tr.className = 'language-option-row';

    const details = document.createElement('td');
    const option = document.createElement('div');
    option.className = 'language-option';

    const flag = document.createElement('img');
    flag.className = 'language-option-flag';
    flag.src = language.flag;
    flag.alt = '';
    flag.width = 42;
    flag.height = 28;

    const copy = document.createElement('div');
    const name = document.createElement('strong');
    name.className = 'setting-title';
    name.textContent = language.name;

    const metadata = document.createElement('small');
    metadata.className = 'calibri';
    metadata.textContent = `${language.author} · v${language.version}`;

    copy.append(name, document.createElement('br'), metadata);
    option.append(flag, copy);
    details.appendChild(option);

    const { td: action, control } = createSettingControlCell();
    const button = document.createElement('button');
    button.type = 'button';
    button.disabled = selected;
    button.textContent = selected
        ? localize('language_current', 'Current language')
        : localize('select', 'Select');
    button.addEventListener('click', async () => {
        window._pageArguments = { cat: 'lang' };
        await window.Localization.setLanguage(language.code, { refreshPage: false });
        document.querySelector('.page-heading h1').textContent = localize('options', 'Options');
        document.querySelector('.page-heading p').textContent = localize(
            'community_options_subtitle',
            'Configure Community without changing the official Deltamod profile.'
        );
        const categoryLabels = {
            b_gen: ['optcat_general', 'General'],
            b_lang: ['optcat_lang', 'Language'],
            b_ui: ['optcat_ui', 'Interface'],
            b_inst: ['optcat_installation', 'Installation'],
            b_data: ['optcat_data', 'Data'],
            b_adv: ['optcat_advanced', 'Advanced'],
            b_gb: ['optcat_gamebanana', 'GameBanana'],
            b_nexus: ['optcat_nexus', 'Nexus Mods'],
            b_dev: ['optcat_developer', 'Developer']
        };
        for (const [id, [key, fallback]] of Object.entries(categoryLabels)) {
            const category = document.getElementById(id);
            if (category) category.textContent = localize(key, fallback);
        }
        const headers = document.querySelectorAll('#modtable thead th');
        if (headers[0]) headers[0].textContent = localize('option', 'Option');
        if (headers[1]) headers[1].textContent = localize('status', 'Status');
        document.getElementById('pageTitle').textContent = localize('options', 'Options');
        await window.currentPageStack.cat('lang');
    });
    control.appendChild(button);

    tr.append(details, action);
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
        if (choice === 'restart') await window.deltamodBackend.invoke('restartCommunity', []);
    } catch (error) {
        current.innerText = error.code === 'IMPORT_CANCELLED'
            ? 'Import cancelled. Community staging data was removed.'
            : `Import failed: ${error.message || error}`;
        cancel.remove();
    }
}

var tempLock = false;
var pendingCategory = null;

window.currentPageStack.cat = async function(cat) {
    if (tempLock) {
        pendingCategory = cat;
        return;
    }
    tempLock = true;
    let tbody = document.querySelector('tbody');
    tbody.innerHTML = '';

    document.getElementById('b_gen').classList.remove('selected');
    document.getElementById('b_lang').classList.remove('selected');
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
        btn.setAttribute('aria-current', btn.id === 'b_' + cat ? 'page' : 'false');
        if (btn.id != 'b_' + cat) {
            btn.classList.add('blur');
        }
        else {
            btn.classList.remove('blur');
        }
    });
    try {
    switch (cat) {
        case 'gen':            
            await addButton("Open mod folder", "Open the folder where your mods are stored.", async () => {
                await window.deltamodBackend.invoke('openSysFolder', ['mods']);
            }, "Open");
            await addButton(
                localize('community_delete_data_title', "Delete all Community data"),
                localize('community_delete_data_desc', "Deletes Community installations, mods, and options. Official Deltamod data is not changed."),
                async () => {
                page('deleteall');
                },
                "Delete",
                true,
                '',
                'red'
            );
            await addCheckboxOption("Prompt controller mode when available", "When enabled, you will be asked to activate Controller Mode when a compatible controller is attached. Currently only compatible with DualSense.", 'CONTROLLER');
            await addCheckboxOption(
                localize('community_hash_title', "Enable hash checks"),
                localize('community_hash_desc', "Checks mod hashes for compatibility. This may make scans slower."),
                'hashchecks',
                false
            );
            break;
        case 'lang': {
            await addRowHeader(`${icon('language', '20px')} ${localize('optcat_lang', 'Language')}`);
            await addInfoRow(
                localize('language_current', 'Current language'),
                window.Localization.getLanguage().toUpperCase(),
                localize(
                    'language_help',
                    'Choose the language used by Deltamod Community. New Community features may fall back to English until their translations are updated.'
                )
            );

            const languages = await window.Localization.getLanguages();
            const currentLanguage = window.Localization.getLanguage();
            for (const language of languages) {
                await addLanguageOption(language, language.code === currentLanguage);
            }
            break;
        }
        case 'ui':
            await addCheckboxOption("Enable music in menus", "Plays background music in the main menus.", 'audio', false, async (enabled) => {
                if (enabled) {
                    var a = new Audio();
                    a.src = 'audio/orch1.mp3';
                    a.playbackRate = 1.3;
                    a.play();
                    if (themeUsesIntegratedVideoAudio()) {
                        setThemeVideoAudioEnabled(true);
                    } else {
                        currentAudio = "";
                        await page(pageN);
                    }
                }
                else {
                    releaseAudioBuffer();
                    setThemeVideoAudioEnabled(false);
                }
            });
            addRangeOption(
                localize('community_music_volume_title', 'Music volume'),
                localize('community_music_volume_desc', 'Adjusts the volume of menu and theme music.'),
                {
                    value: Math.round((window.DeltamodAudioSettings?.getVolume() ?? 0.5) * 100),
                    changeHandler: value => window.DeltamodAudioSettings?.setVolume(value / 100)
                }
            );
            await addCheckboxOption("Enable SFX in menus", "Plays sound effects in the main menus.", 'sfx', false, (enabled) => {
                if (enabled) {
                    var a = new Audio();
                    a.src = 'audio/orch1.mp3';
                    a.playbackRate = 1.1;
                    a.play();
                }
            });
            await addCheckboxOption(
                localize('community_dynamic_music_title', "Enable dynamic music"),
                localize(
                    'community_dynamic_music_desc',
                    "Enables dynamic background music that changes based on the page. If unchecked, always plays the default music for your theme."
                ),
                'dynamusic',
                true
            );

            await addSelectOption(
                localize('community_alert_alignment', "Alert alignment"),
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

            const seasonalModeSelect = await addSelectOption(
                localize('community_seasonal_title', 'Seasonal details'),
                localize(
                    'community_seasonal_desc',
                    'Adds calendar-based pixel details without replacing the active theme. Choose an event to preview it.'
                ),
                [
                    { value: 'auto', label: localize('seasonal_auto', 'Automatic') },
                    { value: 'off', label: localize('seasonal_off', 'Off') },
                    { value: 'womens-health', label: localize('seasonal_womens_health', "Women's Health") },
                    { value: 'mens-health', label: localize('seasonal_mens_health', "Men's Health") },
                    { value: 'easter', label: localize('seasonal_easter', 'Easter') },
                    { value: 'halloween', label: localize('seasonal_halloween', 'Halloween') },
                    { value: 'christmas', label: localize('seasonal_christmas', 'Christmas') },
                    { value: 'new-year', label: localize('seasonal_new_year', 'New Year') }
                ],
                false,
                value => window.SeasonalEvents?.setMode(value),
                window.SeasonalEvents?.getMode() || 'auto'
            );
            seasonalModeSelect.id = 'SELECT-SEASONAL-MODE';

            break;
        case 'inst':
            var isSteam = await window.deltamodBackend.invoke('isCurrentIndexSteam', []);
            const canDisconnectSteam = window.deltamodBackend.isCommandAvailable('removeSteamIntegration');

            await addButton("Disconnect Steam", "Stops launching the current Community installation through Steam.", async () => {
                const removed = await window.deltamodBackend.invoke('removeSteamIntegration', []);
                if (removed) await window.currentPageStack.cat('inst');
            }, "Disconnect", isSteam && canDisconnectSteam, canDisconnectSteam
                ? "Only available for games imported from Steam."
                : "Steam disconnection is unavailable in this app build.");

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
            const canRebootDev = window.deltamodBackend.isCommandAvailable('rebootDev');

            await addButton("Reboot in Developer Mode", "Reboots in developer mode, a mode which allows you to use the DevTools.", async () => {
                var goOn = await htmlAlert(
                        'Warning', 
                        "Warning: this is only for users who know what they're doing. Are you sure you want to reboot in developer mode?", 
                        [{text:"Yes",resolveWith:'ok'}, {text:"No",rejectWith:'cancel'}]
                    );
                await window.deltamodBackend.invoke('rebootDev', [])
            }, "Open", canRebootDev && !await window.deltamodBackend.invoke('isDevMode', []), canRebootDev
                ? "You are already in developer mode."
                : "Developer-mode reboot is unavailable in this app build.");

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
                    const result = await window.deltamodBackend.invoke('precalcGameHashes', []);
                    await htmlAlert("Hash cache ready", `Cached ${result.fileCount} game file(s).`, [{text: "OK", resolveWith:''}]);
                } catch (error) {
                    await htmlAlert("Hashing failed", error?.message || 'The game hash cache could not be built.', [{text: "OK", resolveWith:''}]);
                } finally {
                    delete window.currentPageStack.hashProgress;
                    hashButton.disabled = false;
                    hashButton.innerText = 'Build cache';
                }
            }, "Build cache");

            if (window.deltamodBackend.isCommandAvailable('storage:getUsage')) {
                const usage = await window.deltamodBackend.invoke('storage:getUsage', []);
                await addInfoRow('Provider cache', formatProfileBytes(usage.cacheBytes),
                    'Re-downloadable catalogue metadata and images.');
                await addInfoRow('Recovery data', formatProfileBytes(usage.recoveryBytes),
                    'Protected rollback generations; Clear cache never removes these files.');
                await addInfoRow('Lifecycle journals', formatProfileBytes(usage.journalBytes),
                    'Transactional records used for deterministic crash recovery.');
            }
            await addButton(
                'Clear provider cache',
                'Removes only re-downloadable provider catalogue data. Recovery generations and journals are preserved.',
                async () => {
                    const result = await window.deltamodBackend.invoke('storage:clearCache', []);
                    await htmlAlert(
                        'Cache cleared',
                        `Removed ${formatProfileBytes(result.removedBytes)}. Recovery data was not changed.`,
                        [{ text: 'OK', resolveWith: '' }]
                    );
                },
                'Clear cache',
                window.deltamodBackend.isCommandAvailable('storage:clearCache'),
                'Cache cleanup is unavailable in this app build.'
            );
            await addButton(
                'Delete recovery data',
                'Removes only completed recovery generations that are not active, pinned, journal-referenced, among the latest three, or the sole viable recovery for an installation.',
                async () => {
                    const choice = await htmlAlert(
                        'Delete recovery data?',
                        'Older removable rollback generations will be permanently deleted. Active and required recovery data will be preserved.',
                        [
                            { text: 'Delete removable data', resolveWith: 'delete' },
                            { text: 'Cancel', resolveWith: 'cancel' }
                        ],
                        'error'
                    );
                    if (choice !== 'delete') return;
                    const result = await window.deltamodBackend.invoke('storage:deleteRecoveryData', []);
                    await htmlAlert(
                        'Recovery cleanup complete',
                        `Removed ${result.removedGenerations} generation(s), freeing ${formatProfileBytes(result.removedBytes)}. ${result.protectedGenerations} protected generation(s) remain.`,
                        [{ text: 'OK', resolveWith: '' }]
                    );
                    window._pageArguments = { cat: 'adv' };
                    page('options');
                },
                'Delete recovery data',
                window.deltamodBackend.isCommandAvailable('storage:deleteRecoveryData'),
                'Recovery cleanup is unavailable in this app build.',
                'red'
            );

            await addButton(
                "DeltamodCLI releases",
                "Opens the separate DeltamodCLI project. Community does not automatically execute downloaded installer scripts.",
                async () => window.deltamodBackend.invoke('installDeltamodCLI', []),
                "View releases",
                window.deltamodBackend.isCommandAvailable('installDeltamodCLI'),
                "DeltamodCLI release launching is unavailable in this app build."
            );

            break;
        // dev isnt keyed and is always in english
        case "dev":
            await addRowHeader(icon('warning', '20px') + ' ' + "These options are for developers only.");
            await addButton('Open flag database (DEV-ONLY)', 'Opens the database holding flags.', async () => {
                await window.deltamodBackend.invoke('openFlagDatabase', []);
            }, "Open", window.deltamodBackend.isCommandAvailable('openFlagDatabase'),
                'Flag database launching is unavailable in this app build.');
            await addButton('Force controller mode (DEV-ONLY)', 'Forces Controller Mode on, regardless of controller detection status', async () => {
                await window.deltamodBackend.invoke('cmode-on', []);
            }, "Open", window.deltamodBackend.isCommandAvailable('cmode-on'),
                'Controller mode is unavailable in this app build.');
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
                window.deltamodBackend.invokeOptional(
                    'getGamebananaUserinfo',
                    [],
                    { loggedIn: false }
                ),
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
                    await window.deltamodBackend.invoke('logoutGamebanana', []);
                    window._pageArguments = {cat: 'gb'};
                    page('options');
                }, "Logout", gamebananaUserinfo.loggedIn, "You aren't logged in to GameBanana.", '');
            }
            else {
                await addButton("Login", "Adds a GameBanana account to Deltamod.", async () => {
                    try {
                        const loggedIn = await window.deltamodBackend.invoke('loginGamebanana', []);
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
                }, "Login", !gamebananaUserinfo.loggedIn
                    && window.deltamodBackend.isCommandAvailable('loginGamebanana'),
                window.deltamodBackend.isCommandAvailable('loginGamebanana')
                    ? "You are already logged in to GameBanana."
                    : "GameBanana login is unavailable in this app build.", '');
            }
            break;
        case 'nexus': {
            await addRowHeader(`${icon('key', '20px')} Nexus Mods`);
            const status = await window.communityAPI.modSources.nexusStatus();
            if (status.connected) {
                await addInfoRow('Connection', `Connected as ${status.name}`);
                await addInfoRow('Authentication', 'Nexus Mods OAuth 2.0');
                await addInfoRow(
                    'Download access',
                    status.premium ? 'Premium API downloads available' : 'Website confirmation may be required',
                    status.premium
                        ? 'Compatible archives can be downloaded and imported directly.'
                        : 'Nexus Mods restricts direct API downloads for non-premium accounts.'
                );
                await addButton(
                    'Disconnect Nexus Mods',
                    'Removes the saved Nexus Mods OAuth authorization from this device.',
                    async () => {
                        await window.communityAPI.modSources.clearNexusKey();
                        window._pageArguments = { cat: 'nexus' };
                        page('options');
                    },
                    'Disconnect'
                );
            } else {
                const registrationPending = status.code === 'NEXUS_SSO_NOT_REGISTERED'
                    || status.ssoAvailable !== true;
                const signInPending = status.ssoPending === true;
                await addInfoRow(
                    'Connection',
                    signInPending
                        ? 'Sign-in pending'
                        : registrationPending ? 'Awaiting Nexus registration' : 'Not connected',
                    signInPending
                        ? 'Finish authorization in the Nexus Mods browser window, or cancel the pending sign-in below.'
                        : registrationPending
                            ? 'Nexus Mods must issue the OAuth client ID before Community can offer account sign-in.'
                            : (status.error || 'Connect a Nexus Mods account with OAuth to browse its catalogue.')
                );
                if (status.ssoAvailable === true) {
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
                        'Opens Nexus Mods in your browser and returns through Community’s fixed local callback.',
                        toggleSso,
                        status.ssoPending ? 'Cancel sign-in' : 'Sign in'
                    );
                    signingIn = Boolean(status.ssoPending);
                } else if (!signInPending) {
                    await addInfoRow(
                        'OAuth 2.0',
                        'Unavailable',
                        'This integration is waiting for the Nexus-issued OAuth client ID. No credential can be entered manually.'
                    );
                }
            }
            break;
        }
    }
    } catch (error) {
        console.error(`Unable to render options category ${cat}:`, error);
        tbody.replaceChildren();
        const row = document.createElement('tr');
        const cell = document.createElement('td');
        cell.colSpan = 2;
        cell.innerText = `This options category could not be loaded: ${String(error?.message || error)}`;
        row.appendChild(cell);
        tbody.appendChild(row);
    } finally {
        // as far as i know this page is the only page that needs ts
        genbtnstyles();
        rew();
        tempLock = false;
        if (pendingCategory) {
            const nextCategory = pendingCategory;
            pendingCategory = null;
            if (nextCategory !== cat) window.currentPageStack.cat(nextCategory);
        }
    }
}

if (window._pageArguments?.cat != undefined && typeof window.currentPageStack?.cat === 'function') {
    const selectedCategory = window._pageArguments.cat;
    window._pageArguments = {};
    window.currentPageStack.cat(selectedCategory);
} else {
    window.currentPageStack.cat('gen');
}
})();
