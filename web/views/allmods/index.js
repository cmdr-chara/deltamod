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
    const max = 150;
    if (text.length > max) return text.substring(0, max) + '...';
    return text;
}

function purify(text) {
    return text.replace(/<[^>]*>/g, '');
}

function setIconText(element, iconName, text, size = 'small') {
    element.innerHTML = icon(iconName, size);
    element.appendChild(document.createTextNode(` ${String(text ?? '')}`));
}

async function createMod(mod, compatible, loggedIn, modListElement) {
    const modRow = document.createElement('tr');

    let imeta = await window.deltamodBackend.invoke('getModImage', [mod.uid]);
    if (!imeta.path) {
        imeta.path = window.deltamodBackend.assetUrl('app', 'web/img/mod-placeholder.png');
    }

    // Column 1 (Mod)
    const modNameContainer = document.createElement('td');
    const titleSpan = document.createElement('div');
    const modImage = document.createElement('img');
    modImage.src = imeta.path;
    modImage.alt = '';
    modImage.width = 32;
    modImage.height = 32;
    modImage.style.borderRadius = '4px';
    modImage.style.objectFit = 'cover';
    modImage.onerror = () => {
        modImage.onerror = null;
        modImage.src = window.deltamodBackend.assetUrl('app', 'web/img/mod-placeholder.png');
    };
    const modTitle = document.createElement('span');
    modTitle.textContent = String(mod.name || 'Unnamed mod');
    titleSpan.append(modImage, modTitle);
    titleSpan.style.display = 'flex';
    titleSpan.style.alignItems = 'center';
    titleSpan.style.gap = '8px';
    titleSpan.style.marginBottom = '4px';
    titleSpan.id = `modtitle-${mod.uid}`;
    modNameContainer.appendChild(titleSpan);

    if (window._pageArguments && window._pageArguments.highlightMod === mod.uid) {
        modNameContainer.style.backgroundColor = '#b5b5b544';
        setTimeout(() => {
            try {
                modNameContainer.scrollIntoView({ behavior: 'auto', block: 'center', inline: 'nearest' });
            } catch (e) {
                modNameContainer.scrollIntoView();
            }
        }, 50);
    }

    const descSpan = document.createElement('span');
    descSpan.className = 'calibri';
    descSpan.style = 'font-size: 10px; color: #ffffffdd;';
    descSpan.innerText = purifyDescription(mod.description);
    descSpan.id = `moddesc-${mod.uid}`;
    modNameContainer.appendChild(descSpan);

    let authorSpan = document.createElement('p');
    authorSpan = adaptForIcons(authorSpan);
    authorSpan.style.margin = '0px';
    authorSpan.style.marginTop = '4px';
    authorSpan.className = 'calibri';
    authorSpan.style.fontSize = 'smaller';
    authorSpan.style.color = '#888';
    setIconText(authorSpan, 'attribution', Array.isArray(mod.author) ? mod.author.join(', ') : mod.author);
    authorSpan.id = `modauthor-${mod.uid}`;
    modNameContainer.appendChild(document.createElement('br'));
    modNameContainer.appendChild(authorSpan);

    let sizeSpan = document.createElement('p');
    sizeSpan = adaptForIcons(sizeSpan);
    sizeSpan.style.margin = '0px';
    sizeSpan.style.marginTop = '4px';
    sizeSpan.className = 'calibri';
    sizeSpan.style.fontSize = 'smaller';
    sizeSpan.style.color = '#888';
    setIconText(sizeSpan, 'hard_disk', `${mod.size} MB`);
    sizeSpan.id = `modsize-${mod.uid}`;
    modNameContainer.appendChild(sizeSpan);

    let idSpan = document.createElement('p');
    idSpan = adaptForIcons(idSpan);
    idSpan.style.margin = '0px';
    idSpan.style.marginTop = '4px';
    idSpan.className = 'calibri';
    idSpan.style.fontSize = 'smaller';
    idSpan.style.color = '#888';
    setIconText(idSpan, 'sell', mod.packageID == 'und.und.und' ? "No ID was specified." : mod.packageID);
    idSpan.id = `modid-${mod.uid}`;
    modNameContainer.appendChild(idSpan);

    let gameSpan = document.createElement('p');
    gameSpan = adaptForIcons(gameSpan);
    gameSpan.style.margin = '0px';
    gameSpan.style.marginTop = '4px';
    gameSpan.className = 'calibri';
    gameSpan.style.fontSize = 'smaller';
    gameSpan.style.color = '#888';
    setIconText(gameSpan, 'stadia_controller', await window.deltamodBackend.invoke('getGameInfo', [mod.game]).then(g => g.name));
    gameSpan.id = `modgame-${mod.uid}`;
    modNameContainer.appendChild(gameSpan);

    let versionSpan = document.createElement('p');
    versionSpan = adaptForIcons(versionSpan);
    versionSpan.style.margin = '0px';
    versionSpan.style.marginTop = '4px';
    versionSpan.className = 'calibri';
    versionSpan.style.fontSize = 'smaller';
    versionSpan.style.color = '#888';
    setIconText(versionSpan, 'change_history', mod.version);
    versionSpan.id = `modversion-${mod.uid}`;
    modNameContainer.appendChild(versionSpan);

    if ((mod.variants || []).length > 0) {
        let variantSpan = document.createElement('p');
        variantSpan = adaptForIcons(variantSpan);
        variantSpan.style.margin = '0px';
        variantSpan.style.marginTop = '4px';
        variantSpan.className = 'calibri';
        variantSpan.style.fontSize = 'smaller';
        variantSpan.style.color = '#888';
        setIconText(variantSpan, 'stack', `Mod has ${mod.variants.length} variants`);
        variantSpan.id = `modvariant-${mod.uid}`;
        modNameContainer.appendChild(variantSpan);
    }

    var comp = !mod.isIncompatible;
    let compatSpan = document.createElement('p');
    compatSpan = adaptForIcons(compatSpan);
    compatSpan.style.margin = '0px';
    compatSpan.style.marginTop = '4px';
    compatSpan.className = 'calibri';
    compatSpan.style.fontSize = 'smaller';
    compatSpan.style.color = comp ? '#4caf50' : '#f44336';
    setIconText(
        compatSpan,
        comp ? 'check' : 'error',
        comp ? "Compatible with current version" : `Incompatible: ${mod.incompatibilityReason}`
    );
    compatSpan.id = `modcompat-${mod.uid}`;
    modNameContainer.appendChild(compatSpan);
    
    if (mod.gamebanana.supports) {
        let gbSpan = document.createElement('p');
        gbSpan = adaptForIcons(gbSpan);
        gbSpan.style.margin = '0px';
        gbSpan.style.marginTop = '4px';
        gbSpan.className = 'calibri';
        gbSpan.style.fontSize = 'smaller';
        gbSpan.style.color = '#888';
        const banana = document.createElement('img');
        banana.src = './img/banana-outline.png';
        banana.alt = '';
        banana.width = 15;
        banana.height = 15;
        gbSpan.append(banana, document.createTextNode(' Installed through GameBanana'));
        gbSpan.id = `modgb-${mod.uid}`;
        modNameContainer.appendChild(gbSpan);
    }
    // Column 2 (Actions)
    const actionContainer = document.createElement('td');
    actionContainer.style.textAlign = 'center';
    actionContainer.className = 'modlist-actions-column';
    {
        let bdiv = document.createElement('div');
        bdiv.className = 'modlist-actions-column-bdiv';
        actionContainer.appendChild(bdiv);

        const exploreModButton = document.createElement('button');
        exploreModButton.onclick = () => window.deltamodBackend.invoke('openModFolder', [mod.folder]);
        exploreModButton.innerHTML = icon('folder_eye', '20px');
        bdiv.appendChild(exploreModButton);

        const deleteModButton = document.createElement('button');
        deleteModButton.onclick = () => {
            window.deltamodBackend.invoke('removeMod', [mod.folder]);
        };
        deleteModButton.innerHTML = icon('delete_forever', '20px');
        bdiv.appendChild(deleteModButton);

            const gbModButton = document.createElement('button');
            gbModButton.onclick = () => {
                window._pageArguments = {
                    id: mod.gamebanana.id,
                    model: mod.gamebanana.model
                };
                page(`gamebanana-leave-comment`);
            };
            gbModButton.innerHTML = icon('comment', '20px');
            bdiv.appendChild(gbModButton);

            const likeBtn = document.createElement('button');
            likeBtn.onclick = async () => {
                let res = await window.deltamodBackend.invoke('gbLikeMod',[mod.gamebanana.model, mod.gamebanana.id]);
                    if (res.status == 200) {
                        likeBtn.innerHTML = icon('sentiment_very_satisfied', '20px') + '';
                        likeBtn.disabled = true;
                    }
                    else if (res.data._sErrorCode.toLowerCase() == 'already_liked') {
                        await htmlAlert("Can't like mod","You've already liked this mod. Can't get any more likes than that!",[{text:"Ok",resolveWith:'ok'}], 'sentiment_very_satisfied');
                        likeBtn.innerHTML = icon('sentiment_very_satisfied', '20px') + '';
                        likeBtn.disabled = true;
                    } else {
                        await htmlAlert("Can't like mod",res.data._sErrorCode,[{text:"Ok",resolveWith:'ok'}], 'error');
                    }
            };
            likeBtn.innerHTML = icon('mood_heart', '20px');
            bdiv.appendChild(likeBtn);

            tippy(likeBtn, {
                content: "Like this mod on GameBanana",
            });
            tippy(gbModButton, {
                content: loggedIn ? "Leave a comment on GameBanana" : "View the GameBanana comments for this mod",
            });

            likeBtn.disabled = !mod.gamebanana.supports || !loggedIn;
            gbModButton.disabled = !mod.gamebanana.supports;
    }

    modRow.appendChild(modNameContainer);
    modRow.appendChild(actionContainer);

    if (!modListElement.isConnected) return null;
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

(async () => {
    const errorBanner = document.getElementById("error-banner");
    const gamesShowSelect = document.getElementById('gamesShow');
    const modListElement = document.getElementById('modlist');
    if (!errorBanner || !gamesShowSelect || !modListElement) return;
    const isPageActive = () =>
        errorBanner.isConnected &&
        gamesShowSelect.isConnected &&
        modListElement.isConnected;

    var loggedIn = await window.deltamodBackend.invoke('validateGamebananaToken', []);
    if (!isPageActive()) return;
    const pageArguments = window._pageArguments || {};
    const selectedSpecID = pageArguments.specID;

    let filterFunc = (x) => true;
    if (selectedSpecID != undefined && selectedSpecID !== 'all') {
        filterFunc = (mod) => mod.game === selectedSpecID;
    }

    var enumerateGames = await window.deltamodBackend.invoke('getAvailableGames', []);
    if (!isPageActive()) return;
    for (const game of enumerateGames) {
        const option = document.createElement('option');
        option.value = game.id;
        option.innerText = game.name;
        gamesShowSelect.appendChild(option);
    }

    gamesShowSelect.onchange = () => {
        const selectedGame = gamesShowSelect.value;
        window._pageArguments = { specID: selectedGame };
        page('allmods');
    };
    
    if (selectedSpecID != undefined && selectedSpecID !== 'all') {
        gamesShowSelect.value = selectedSpecID;
    }
    

    var { modList, errors } = await window.deltamodBackend.invoke('getModList', []);
    if (!isPageActive()) return;

    var list = modList.filter(filterFunc);
    for (const mod of list) {
        const modRow = await createMod(mod, mod.isCompatible, loggedIn, modListElement);
        if (!modRow) return;
    }
    window._pageArguments = {}; // Clear it so it doesn't affect other mods

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

    if (list.length === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 2;
        td.className = 'empty-state-cell';

        const state = document.createElement('div');
        state.className = 'empty-state';
        const stateIcon = document.createElement('img');
        stateIcon.className = 'empty-state-icon';
        stateIcon.setAttribute('aria-hidden', 'true');
        stateIcon.src = modList.length === 0 ? './sbar/allmods.png' : './sbar/installmanager.png';
        stateIcon.alt = '';

        const copy = document.createElement('div');
        copy.className = 'empty-state-copy';
        const heading = document.createElement('h2');
        heading.innerText = modList.length === 0
            ? t('allmods_empty_title', 'No installed mods yet')
            : 'No mods match this installation';
        const description = document.createElement('p');
        description.innerText = modList.length === 0
            ? t(
                'allmods_empty_desc',
                'Packages you download or import will stay visible here, even when they are not enabled in the current patch list.'
            )
            : 'Choose another installation above or return to All installations.';
        copy.append(heading, description);

        if (modList.length === 0) {
            const actions = document.createElement('div');
            actions.className = 'empty-state-actions';
            const shopButton = document.createElement('button');
            shopButton.innerText = t('browse_mod_shop', 'Browse Mod Shop');
            shopButton.onclick = () => page('gamebanana-browse');
            const importButton = document.createElement('button');
            importButton.className = 'secondary-action';
            importButton.innerText = t('import_mod_package', 'Import mod package');
            importButton.onclick = () => window.deltamodBackend.invoke('importMod', []);
            actions.append(shopButton, importButton);
            copy.appendChild(actions);
        }

        state.append(stateIcon, copy);
        td.appendChild(state);
        tr.appendChild(td);
        modListElement.appendChild(tr);
    }

    genbtnstyles();
})();
})();
