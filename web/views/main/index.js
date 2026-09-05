(() => {
const t = (key, fallback, ...args) =>
    window.Localization?.t(key, fallback, ...args) ?? fallback;

const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
function purifyDescription(desc) {
    if (desc === null || desc === undefined) return '';
    let text = String(desc);
    // Remove any HTML tags first
    text = purify(text);
    // Normalize whitespace/newlines to single spaces
    text = text.replace(/\s+/g, ' ').trim();
    // Only add some words
    const maxWords = 25;
    const words = text.split(' ').slice(0, maxWords);
    text = words.join(' ') + (words.length >= maxWords ? '...' : '');
    // If too long, truncate as last resort
    const max = 100;
    if (text.length > max) return text.substring(0, max) + '...';
    return text;
}

var noMergeMods = [];
const pageTable = document.getElementById('modlist');
const thumbnails = window.DeltamodUI.thumbnailLoader();
let pendingToggles = 0;
let launching = false;
const launchButton = document.getElementById('par');
const updateLaunchState = () => { if (launchButton?.isConnected) launchButton.disabled = launching || pendingToggles > 0; };

function adaptForIconsA(elem) {
    elem.style.display = 'inline-flex';
    elem.style.alignItems = 'center';
    elem.style.gap = '4px';
    elem.style.justifyContent = 'left';
    return elem;
}
function purify(text) {
    return text.replace(/<[^>]*>/g, '');
}

function setIconText(element, iconName, text, size = 'small') {
    element.innerHTML = icon(iconName, size);
    element.appendChild(document.createTextNode(` ${String(text ?? '')}`));
}

async function createMod(mod, modListElement) {
    let modRow = document.createElement('tr');

    modRow.className = 'modrow';
    modRow.dataset.uid = mod.uid;
    modRow.dataset.search = [mod.name, mod.author, mod.version, mod.packageID].flat().filter(Boolean).join(' ');

    // Column 1 (Mod)
    let modNameContainer = document.createElement('td');

    let bigAhhContainer = document.createElement('div');
    bigAhhContainer.className = 'patch-mod-layout';

    let imageContainer = document.createElement('button');
    imageContainer.type = 'button';
    imageContainer.setAttribute('aria-label', `${t('ui_details', 'View in library')}: ${mod.name}`);
    imageContainer.title = t('ui_details', 'View in library');
    imageContainer.onclick = () => { window._pageArguments = { highlightMod: mod.uid }; page('allmods'); };
    imageContainer.className = 'patch-mod-artwork secondary-action';

    let img = document.createElement('img');
    img.classList.add('mod-image');
    img.alt = '';
    thumbnails.add(img, mod);
    imageContainer.appendChild(img);

    imageContainer.oncontextmenu = async e => {
        e.preventDefault();
        htmlAlert(mod.name,"Do you wish to view this mod in the Library?",[{text:"Yes",resolveWith:'accept'},{text:"No",rejectWith:'close'}]).then(result => {
            if (result === 'accept') {
                window._pageArguments = { highlightMod: mod.uid };
                page('allmods');
            }
        }).catch(() => {});
    };


    let infoContainer = document.createElement('div');
    infoContainer.className = 'patch-mod-info';
    let titleSpan = document.createElement('span');
    titleSpan.innerText = mod.name;
    titleSpan.className = 'patch-mod-title';
    if (mod.new) {
        titleSpan = adaptForIconsA(titleSpan);
        titleSpan.style.marginBottom = '0px';
        setIconText(titleSpan, 'fiber_new', mod.name, '30px');
    }
    titleSpan.id = `modtitle-${mod.uid}`;
    infoContainer.appendChild(titleSpan);

    let descSpan = document.createElement('span');
    descSpan.className = 'calibri patch-mod-description';
    descSpan.innerText = purifyDescription(mod.description);
    descSpan.id = `moddesc-${mod.uid}`;
    infoContainer.appendChild(descSpan);

    let flexContnainer = document.createElement('div');
    flexContnainer.className = 'patch-mod-meta';
    infoContainer.appendChild(flexContnainer);

    const authors = Array.isArray(mod.author) ? mod.author : [mod.author || 'Unknown'];
    var reducedAuthorStr = authors.slice(0,2).join(', ');
    if (authors.length > 2) reducedAuthorStr += ` and ${authors.length - 2} more`;
    var fontSize = 13;
    let authorSpan = document.createElement('p');
    authorSpan = adaptForIconsA(authorSpan);
    authorSpan.style.margin = '0px';
    authorSpan.className = 'calibri';
    setIconText(authorSpan, 'attribution', reducedAuthorStr, fontSize + 'px');
    authorSpan.id = `modauthor-${mod.uid}`;
    flexContnainer.appendChild(authorSpan);

    let versionSpan = document.createElement('p');
    versionSpan = adaptForIconsA(versionSpan);
    versionSpan.style.margin = '0px';
    versionSpan.className = 'calibri';
    setIconText(versionSpan, 'change_history', mod.version || "Unknown", fontSize + 'px');
    versionSpan.id = `modsize-${mod.uid}`;
    flexContnainer.appendChild(versionSpan);

    if (!mod.mergeSupport) {
        noMergeMods.push({uid: mod.uid, name: mod.name});  

        let mergeSpan = document.createElement('p');
        mergeSpan = adaptForIconsA(mergeSpan);
        mergeSpan.className = 'calibri patch-mod-warning';
        setIconText(mergeSpan, 'warning', "This mod is incompatible with multiple mod support", fontSize + 'px');
        mergeSpan.id = `modmerge-${mod.uid}`;
        infoContainer.appendChild(mergeSpan);
    }

    if (mod.variants != null) {
        let variantSelect = document.createElement('select');
        variantSelect.className = 'variant-select calibri patch-mod-variant';
        variantSelect.setAttribute('aria-label', `Variant: ${mod.name}`);
        for (const variant of mod.variants) {
            let option = document.createElement('option');
            option.value = variant.filename;
            option.innerText = variant.name;
            if (variant.filename == null || variant.filename == undefined || variant.name == null || variant.name == undefined) {
                continue;
            }
            variantSelect.appendChild(option);
        }
        if (variantSelect.children.length != 0) {
            variantSelect.onchange = e => {
                const selectedVariant = variantSelect.value;
                window.deltamodBackend.invoke('setModVariant', [selectedVariant, mod.folder]);
            };
            variantSelect.value = mod._selectedVariant || mod.variants[0].filename;
            if (mod._selectedVariant == null || !mod.variants.some(v => v.filename === mod._selectedVariant)) {
                window.deltamodBackend.invoke('setModVariant', [mod.variants[0].filename, mod.folder]);
            }
            infoContainer.appendChild(variantSelect);
        }
    }

    bigAhhContainer.appendChild(imageContainer);
    bigAhhContainer.appendChild(infoContainer);

    modNameContainer.appendChild(bigAhhContainer);

    // Column 2 (Actions)
    let enabledContainer = document.createElement('td');
    enabledContainer.className = 'modlist-enabled-column';
    {
        let toggleLabel = document.createElement('label');
        toggleLabel.className = 'patch-toggle';

        let enabled = document.createElement("input");
        enabled.type = 'checkbox';
        enabled.id = `modcheck-${mod.uid}`;
        enabled.setAttribute('aria-label', `Enable ${mod.name}`);
        enabled.checked = typeof mod._enabled === 'boolean' ? mod._enabled : await window.deltamodBackend.invoke('getModState', [mod.uid]);
        modRow.classList.toggle('is-enabled', enabled.checked);
        enabled.onchange = async e => {
            const c = e.target;
            const isEnabled = c.checked;
            pendingToggles += 1; c.disabled = true; updateLaunchState();
            modRow.classList.toggle('is-enabled', isEnabled);
            try {
                await window.deltamodBackend.invoke('toggleModState', [mod.uid, isEnabled]);
            } catch (error) {
                c.checked = !isEnabled;
                modRow.classList.toggle('is-enabled', !isEnabled);
                if (modRow.isConnected) await htmlAlert('Unable to change mod state', String(error?.message || error), [{ text: 'OK' }]);
            } finally {
                pendingToggles -= 1; c.disabled = false; updateLaunchState();
            }
        };

        let toggleTrack = document.createElement('span');
        toggleTrack.className = 'patch-toggle-track';
        toggleTrack.setAttribute('aria-hidden', 'true');
        toggleLabel.appendChild(enabled);
        toggleLabel.appendChild(toggleTrack);
        enabledContainer.appendChild(toggleLabel);
    }

    modRow.appendChild(modNameContainer);
    modRow.appendChild(enabledContainer);

    if (!modListElement?.isConnected) return null;
    modListElement.appendChild(modRow);
    return modRow;
}

async function createErroringMods(errors) {
    const dialogElement = document.getElementById("error-list-dialog");
    const errorList = document.getElementById("error-list-div");

    for (const child of errorList.children) errorList.removeChild(child);

    for (const err of errors) {
        // err { mod: string, reason: string }
        const element = document.createElement("div");
        element.className = "error-holder";

        const modId = document.createElement("span");
        modId.textContent = `Mod ID '${String(err.mod || '')}'`;
        modId.style.fontSize = '20px';
        modId.style.color = '#888';

        const reasoning = document.createElement("span");
        reasoning.className = 'calibri';
        setIconText(reasoning, 'warning', err.reason, '20px');
        reasoning.style.display = 'flex';
        reasoning.style.alignItems = 'center';
        reasoning.style.gap = '8px';
        reasoning.style.justifyContent = 'left';

        var selectSpan = document.createElement('span');
        selectSpan.className = 'calibri';
        selectSpan.style.marginTop = '18px';
        selectSpan.style.display = 'block';
        selectSpan.innerText = t('modFail_howtoproceed', "How do you want to proceed?");
        

        const actionRow = document.createElement("div");
        actionRow.className = "error-buttons";
        {
            // Action Row
            const exploreBtn = document.createElement("button");
            exploreBtn.innerText = t('open_mod_folder', "Open mod folder");
            exploreBtn.onclick = () => window.deltamodBackend.invoke("openModFolder", [err.mod]);
            actionRow.appendChild(exploreBtn);

            const deleteBtn = document.createElement("button");
            deleteBtn.innerText = t('delete_mod', "Delete mod");
            deleteBtn.onclick = () => window.deltamodBackend.invoke("removeMod", [err.mod]);
            actionRow.appendChild(deleteBtn);
        }

        element.appendChild(modId);
        element.appendChild(document.createElement("br"));
        element.appendChild(reasoning);
        element.appendChild(selectSpan);
        element.appendChild(actionRow);
        errorList.appendChild(element);
    }

    dialogElement.showModal();
}

function loadInst(index) {
    window.deltamodBackend.invoke('changeSystemIndex', ["" + index])
}

(async () => {
    const errorBanner = document.getElementById("error-banner");

    var { modList, errors } = (await window.deltamodBackend.invoke('getModList', []));
    const modListElement = document.getElementById('modlist');
    const sortWay = document.getElementById('sortWay');
    const pageIsActive = () => (
        window.pageN === 'main'
        && modListElement?.isConnected
        && sortWay?.isConnected
    );
    if (!pageIsActive()) return;
    modListElement.replaceChildren();

    if (window._pageArguments && window._pageArguments.sortfunc && window._pageArguments.sortid) {
        modList = modList.sort(window._pageArguments.sortfunc);
        sortWay.value = window._pageArguments.sortid;
    }
    else {
        // sort by name ascending by default
        modList = modList.sort((a, b) => a.name.localeCompare(b.name));
    }
    let rendered = 0;
    for (const mod of modList.filter(item => !item.isIncompatible)) {
        await createMod(mod, modListElement);
        if (!pageIsActive()) return;
        if (++rendered % 32 === 0) await new Promise(requestAnimationFrame);
    }

    const models = new Map(modList.map(mod => [String(mod.uid), mod]));
    const rows = [...modListElement.querySelectorAll('.modrow')];
    const author = mod => String((Array.isArray(mod.author) ? mod.author[0] : mod.author) || '');
    sortWay.onchange = () => {
        const compare = (a, b) => {
            const x = models.get(a.dataset.uid), y = models.get(b.dataset.uid);
            switch (sortWay.value) {
                case 'desc': return String(y.name).localeCompare(String(x.name));
                case 'size-asc': return (x.size || 0) - (y.size || 0);
                case 'size-desc': return (y.size || 0) - (x.size || 0);
                case 'author': return author(x).localeCompare(author(y)) || String(x.name).localeCompare(String(y.name));
                default: return String(x.name).localeCompare(String(y.name));
            }
        };
        const fragment = document.createDocumentFragment();
        rows.sort(compare).forEach(row => fragment.appendChild(row));
        modListElement.replaceChildren(fragment);
    };
    window.DeltamodUI.bindSearch({
        input: document.getElementById('mod-search'), rows,
        output: document.getElementById('mod-search-count'), empty: document.getElementById('mod-search-empty')
    });
    modListElement.setAttribute('aria-busy', 'false');

    if (errors.length > 0) {
        errorBanner.onclick = () => {
            rew();
            createErroringMods(errors);
        };
        errorBanner.children[0].innerText = t(
            'modFail_bannerTitle',
            '{0} mod(s) failed to load',
            errors.length
        );
        errorBanner.style.display = "inherit";
    } else errorBanner.style.display = "none";

    if (modList.filter(x => !x.isIncompatible).length === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 2;
        td.className = 'empty-state-cell';

        const state = document.createElement('div');
        state.className = 'empty-state';
        const stateIcon = document.createElement('img');
        stateIcon.className = 'empty-state-icon';
        stateIcon.setAttribute('aria-hidden', 'true');
        stateIcon.src = './sbar/main.png';
        stateIcon.alt = '';

        const copy = document.createElement('div');
        copy.className = 'empty-state-copy';
        const heading = document.createElement('h2');
        heading.innerText = t('main_empty_title', 'Your patch list is ready');
        const description = document.createElement('p');
        description.innerText = t(
            'main_empty_desc',
            'Import a compatible mod package or browse the Mod Shop. Installed mods will appear here before anything touches the game files.'
        );
        copy.append(heading, description);

        const incompatibleCount = modList.filter(x => x.isIncompatible).length;
        if (incompatibleCount > 0) {
            const detail = document.createElement('small');
            detail.className = 'empty-state-detail';
            detail.innerText = `${incompatibleCount} incompatible mod(s) are hidden for this installation.`;
            copy.appendChild(detail);
        }

        const installedModCount = await window.deltamodBackend.invoke('howManyMods', []);
        if (!pageIsActive()) return;
        if (installedModCount == 0) {
            const actions = document.createElement('div');
            actions.className = 'empty-state-actions';
            const shopButton = document.createElement('button');
            shopButton.innerText = t('browse_mod_shop', 'Browse Mod Shop');
            shopButton.onclick = () => page('gamebanana-browse');
            actions.appendChild(shopButton);
            const importButton = document.createElement('button');
            importButton.className = 'secondary-action';
            importButton.innerText = t('import_mod_package', 'Import mod package');
            importButton.onclick = () => window.deltamodBackend.invoke('importMod', []);
            actions.appendChild(importButton);
            copy.appendChild(actions);
        }

        state.append(stateIcon, copy);
        td.appendChild(state);
        tr.appendChild(td);
        modListElement.appendChild(tr);

        //document.getElementById('par').innerText = 'Run without patches';
    }

    window._pageArguments = null;

    genbtnstyles();
})().catch(error => window.DeltamodUI.showError(pageTable, error, () => page('main')));

async function patchAndRun() {
    if (launching || pendingToggles > 0) return;
    launching = true; updateLaunchState();
    try {
        const selectedMods = [...pageTable.querySelectorAll('input[type="checkbox"]:checked')]
            .filter(input => input.id.startsWith('modcheck-')).map(input => input.id.slice('modcheck-'.length));
        const exclusive = noMergeMods.find(mod => selectedMods.includes(mod.uid));
        if (exclusive && selectedMods.length > 1) {
            await htmlAlert('Incompatible setting detected', `${exclusive.name} must be used on its own. Deselect the other mods before applying.`, [{ text: 'OK' }]);
            return;
        }
        if (selectedMods.length === 0) await window.deltamodBackend.invoke('startGame', []);
        else {
            await page('patching');
            await window.deltamodBackend.invoke('patchAndRun', [selectedMods]);
        }
    } catch (error) {
        await htmlAlert('Unable to launch the game', String(error?.message || error), [{ text: 'OK' }]);
        if (window.pageN === 'patching') await page('main');
    } finally { launching = false; updateLaunchState(); }
}

window.currentPageStack.patchAndRun = patchAndRun;
})();
