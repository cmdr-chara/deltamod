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

function getPredominantColor(img) {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');

    const width = canvas.width = 256;
    const height = canvas.height = 256;

    ctx.drawImage(img, 0, 0, width, height);

    const imageData = ctx.getImageData(0, 0, width, height);
    const data = imageData.data;

    const colorCount = {};
    for (let i = 0; i < data.length; i += 4) {
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const key = `${r},${g},${b}`;
        colorCount[key] = (colorCount[key] || 0) + 1;
    }

    let top = null;
    let second = null;
    for (const [key, count] of Object.entries(colorCount)) {
        if (!top || count > top.count) {
            second = top;
            top = { key, count };
        } else if (!second || count > second.count) {
            second = { key, count };
        }
    }

    const parseKey = (k) => {
        const [r, g, b] = k.split(',').map(Number);
        return { r, g, b };
    };

    const isBlackOrWhite = ({ r, g, b }, tol = 16) => {
        const isBlack = r <= tol && g <= tol && b <= tol;
        const isWhite = r >= 255 - tol && g >= 255 - tol && b >= 255 - tol;
        return isBlack || isWhite;
    };

    let dominantColor = top ? parseKey(top.key) : { r: 0, g: 0, b: 0 };
    if (top && isBlackOrWhite(dominantColor) && second) {
        dominantColor = parseKey(second.key);
    }

    return dominantColor;
}

function noHTML(elem) {
    const tempDiv = document.createElement('div');
    tempDiv.innerHTML = elem;
    return tempDiv.textContent || tempDiv.innerText || '';
}


async function createMod(mod, modListElement) {
    let modRow = document.createElement('tr');

    modRow.className = 'modrow';

    // Column 1 (Mod)
    let modNameContainer = document.createElement('td');

    let bigAhhContainer = document.createElement('div');
    bigAhhContainer.className = 'patch-mod-layout';

    let imageContainer = document.createElement('div');
    imageContainer.className = 'patch-mod-artwork';

    tippy(imageContainer, {
        content: "Right click to view in library",
        placement: 'right',
        delay: [100, 0],
        onMount(instance) {
            const box = instance.popper.querySelector('.tippy-box');
            box.classList.add('calibri');
            if (box) box.style.border = '3px solid #ffffffff';
        }
    });

    let imeta = await window.electronAPI.invoke('getModImage', [mod.uid]);
    if (!imeta.path) {
        imeta.path = 'deltapack://web/img/mod-placeholder.png';
    }

    let img = document.createElement('img');
    img.src = imeta.path;
    img.classList.add('mod-image');
    img.alt = `${mod.name} cover`;
    img.onerror = () => {
        img.onerror = null;
        img.src = 'deltapack://web/img/mod-placeholder.png';
    };
    imageContainer.appendChild(img);

    imageContainer.oncontextmenu = async e => {
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
                window.electronAPI.invoke('setModVariant', [selectedVariant, mod.folder]);
            };
            variantSelect.value = mod._selectedVariant || mod.variants[0].filename;
            if (mod._selectedVariant == null || !mod.variants.some(v => v.filename === mod._selectedVariant)) {
                window.electronAPI.invoke('setModVariant', [mod.variants[0].filename, mod.folder]);
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
        enabled.checked = await window.electronAPI.invoke('getModState', [mod.uid]);
        modRow.classList.toggle('is-enabled', enabled.checked);
        enabled.onchange = e => {
            const c = e.target;
            const isEnabled = c.checked;
            const forMod = mod.uid;

            modRow.classList.toggle('is-enabled', isEnabled);
            window.electronAPI.invoke("toggleModState", [forMod, isEnabled]);
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
            exploreBtn.onclick = () => window.electronAPI.invoke("openModFolder", [err.mod]);
            actionRow.appendChild(exploreBtn);

            const deleteBtn = document.createElement("button");
            deleteBtn.innerText = t('delete_mod', "Delete mod");
            deleteBtn.onclick = () => window.electronAPI.invoke("removeMod", [err.mod]);
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
    window.electronAPI.invoke('changeSystemIndex', ["" + index])
}

(async () => {
    const errorBanner = document.getElementById("error-banner");

    var { modList, errors } = (await window.electronAPI.invoke('getModList', []));
    const modListElement = document.getElementById('modlist');
    const sortWay = document.getElementById('sortWay');
    const pageIsActive = () => (
        window.pageN === 'main'
        && modListElement?.isConnected
        && sortWay?.isConnected
    );
    if (!pageIsActive()) return;

    if (window._pageArguments && window._pageArguments.sortfunc && window._pageArguments.sortid) {
        modList = modList.sort(window._pageArguments.sortfunc);
        sortWay.value = window._pageArguments.sortid;
    }
    else {
        // sort by name ascending by default
        modList = modList.sort((a, b) => a.name.localeCompare(b.name));
    }
    let addedAuthors = [];
    for (const x of modList.filter(x => !x.isIncompatible)) {
        const primaryAuthor = (Array.isArray(x.author) ? x.author[0] : x.author) || 'Unknown Author';
        if (window._pageArguments && window._pageArguments.sortid === "author" && !addedAuthors.includes(primaryAuthor)) {
            // also create author tr
            var tr = document.createElement('tr');
            var td = document.createElement('td');
            td.colSpan = 3;
            td.style.paddingLeft = '20px';
            td.style.fontSize = '18px';
            td.style.fontWeight = 'bold';
            td.style.backgroundColor = 'rgba(40, 40, 40, 0.05)';
            td.style.color = '#fff';
            td.textContent = `Mods by ${primaryAuthor}`;
            tr.appendChild(td);
            addedAuthors.push(primaryAuthor);
            modListElement.appendChild(tr);
        }
        await createMod(x, modListElement);
        if (!pageIsActive()) return;
    }

    sortWay.onchange = async (e) => {
        switch (e.target.value) {
            case 'asc':
                window._pageArguments = { sortfunc: (a, b) => a.name.localeCompare(b.name), sortid: 'asc' };
                page('');
                break;
            case 'desc':
                window._pageArguments = { sortfunc: (a, b) => b.name.localeCompare(a.name), sortid: 'desc' };
                page('');
                break;
            case 'size-asc':
                window._pageArguments = { sortfunc: (a, b) => (a.size || 0) - (b.size || 0), sortid: 'size-asc' };
                page('');
                break;
            case 'size-desc':
                window._pageArguments = { sortfunc: (a, b) => (b.size || 0) - (a.size || 0), sortid: 'size-desc' };
                page('');
                break;
            case 'author':
                window._pageArguments = { sortfunc: (a, b) => {
                    const authorA = a.author[0] || "Unknown Author";
                    const authorB = b.author[0] || "Unknown Author";
                    return authorA.localeCompare(authorB);
                }, sortid: 'author' };
                page('');
                break;

        }
    };

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
        td.colSpan = 3;
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

        const installedModCount = await window.electronAPI.invoke('howManyMods', []);
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
            importButton.onclick = () => window.electronAPI.invoke('importMod', []);
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
})();

async function patchAndRun() {
    var allChecks = Array.from(document.querySelectorAll('input[type="checkbox"]')).filter(cb => cb.id.startsWith('modcheck-'));
    var selectedMods = allChecks.filter(cb => cb.checked).map(cb => cb.id.replace('modcheck-', ''));
    console.log('Selected mods:', selectedMods);

    var goOn = true;
    for (let i = 0; i < selectedMods.length; i++) {
        const modId = selectedMods[i];
        if (!goOn) break;
        if (noMergeMods.map(x => x.uid).includes(modId) && selectedMods.length > 1) {
            await htmlAlert(
                "Incompatible setting detected",
                `${noMergeMods.find(x => x.uid === modId).name} is not compatible with multiple mod support, but you have multiple mods selected. Please deselect other mods or this mod to continue.`,
                [{ text: "Ok", resolveWith: 'ok' }],
                'join'
            );
            goOn = false;
        }
    }
    if (!goOn) return;

    if (selectedMods.length === 0) {
        window.electronAPI.invoke('startGame', []);
    }
    else {
        page('patching');
        setTimeout(() => {
            window.electronAPI.invoke('patchAndRun', [selectedMods]);
        }, 1000);
    }
}

window.currentPageStack.patchAndRun = patchAndRun;
})();
