(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
let PAGE = (window._pageArguments && window._pageArguments.lp) ? parseInt(window._pageArguments.lp) : 1;
let pageActive = true;
let SHOP_PROVIDER = window._pageArguments?.provider
    || localStorage.getItem('modShopProvider')
    || 'gamebanana';
let SHOP_GAME_ID = window._pageArguments?.gameId
    || localStorage.getItem('modShopGameId')
    || null;
let SHOP_GAME = null;

const SHOP_PROVIDER_DEFS = Object.freeze([
    { id: 'gamebanana', name: 'GameBanana' },
    { id: 'nexus', name: 'Nexus Mods' },
    { id: 'moddb', name: 'ModDB (10 recent)' }
]);

function gameSupportsProvider(game, provider) {
    if (!game || typeof game !== 'object') return false;
    if (provider === 'gamebanana') return Number(game?.gamebanana?.id) > 0;
    if (provider === 'nexus') return Boolean(String(game?.sources?.nexus?.domain || '').trim());
    if (provider === 'moddb') return Boolean(String(game?.sources?.moddb?.slug || '').trim());
    return false;
}

function availableProvidersForGame(game) {
    return SHOP_PROVIDER_DEFS.filter(provider => gameSupportsProvider(game, provider.id));
}

async function resolveShopGame() {
    const games = await window.deltamodBackend.invoke('getAvailableGames', []);
    const supportedGames = (Array.isArray(games) ? games : [])
        .filter(game => SHOP_PROVIDER_DEFS.some(provider => gameSupportsProvider(game, provider.id)));
    if (supportedGames.length === 0) {
        throw new Error('No supported games are available for the Mod Shop.');
    }

    let currentGame = null;
    try {
        currentGame = await window.deltamodBackend.invoke('getCurrentGameInfo', []);
    } catch {
        // Browsing the catalogue does not require an active installation.
    }

    const requestedId = SHOP_GAME_ID || currentGame?.id;
    SHOP_GAME = supportedGames.find(game => game.id === requestedId)
        || supportedGames.find(game => game.id === currentGame?.id)
        || supportedGames[0];
    SHOP_GAME_ID = SHOP_GAME.id;
    localStorage.setItem('modShopGameId', SHOP_GAME_ID);
    return { supportedGames, currentGame };
}

function currentShopGameBananaId() {
    return Number(SHOP_GAME?.gamebanana?.id) || 0;
}

function preserveShopArguments(extra = {}) {
    return { gameId: SHOP_GAME_ID, provider: SHOP_PROVIDER, ...extra };
}

// External catalogue requests are shared across page instances. Navigating or
// changing sort while a Nexus request is in flight must not create another
// request against the same quota window; callers with a different query wait
// for the active request to finish before starting their own.
const externalBrowseState = window._communityExternalBrowseState
    || (window._communityExternalBrowseState = { active: null });
let externalRetryTimer = null;
let gameBananaPageRequestActive = false;
let gameBananaInitialLoadComplete = false;
let gameBananaRetryPending = false;
let gameBananaHasMore = true;
let gameBananaDownloadActive = false;
let gameBananaFeaturedRecordsPromise = null;
const renderedGameBananaRecords = new Set();

window.PAGE = PAGE;

window._onClosePage.push(() => {
    pageActive = false;
    if (externalRetryTimer) {
        clearTimeout(externalRetryTimer);
        externalRetryTimer = null;
    }
    delete window.PAGE;
});

function isCurrentShopPage() {
    return pageActive && window.pageN === 'gamebanana-browse';
}

function timeoutPromise(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

function getThumbURL(mod) {
    try {
        if (mod._sImageUrl && mod._sImageUrl.length > 0) {
            return mod._sImageUrl;
        }
        return mod._aPreviewMedia._aImages[0]?._sBaseUrl + "/" + mod._aPreviewMedia._aImages[0]._sFile530;
    }
    catch {
        return 'https://gamebanana.com/img/gblogo.png';
    }
}

const element = document.querySelector('.scrollBottomDetector');

const observer = new IntersectionObserver((entries) => {
  entries.forEach(entry => {
    if (
        entry.isIntersecting
        && isCurrentShopPage()
        && gameBananaInitialLoadComplete
        && !gameBananaPageRequestActive
    ) {
        plusPage(1);
    }
  });
}, {
  threshold: 0.1
});

if (SHOP_PROVIDER === 'gamebanana') observer.observe(element);

window._onClosePage.push(() => {
    observer.disconnect();
});

function getAllThumbs(mod) {
    const images = Array.isArray(mod?._aPreviewMedia?._aImages) ? mod._aPreviewMedia._aImages : [];
    let ar = images.map(x => {
        const baseUrl = x._sBaseUrl + "/";
        const file220 = x._sFile220 || x._sFile530 || x._sFile;
        const file530 = x._sFile530 || x._sFile220 || x._sFile;
        return {
            urlA: baseUrl + x._sFile,
            urlB: baseUrl + (x._sFile100 || file220),
            urlCard220: baseUrl + file220,
            urlCard530: baseUrl + file530
        }
    });
    if (ar.length === 0) {
        const imageUrl = mod?._sImageUrl || mod?._sThumbnailUrl;
        ar.push({
            urlA: imageUrl || './img/mod-placeholder.png',
            urlB: mod?._sThumbnailUrl || imageUrl || './img/mod-placeholder.png',
            urlCard220: mod?._sThumbnailUrl || imageUrl || './img/mod-placeholder.png',
            urlCard530: imageUrl || mod?._sThumbnailUrl || './img/mod-placeholder.png'
        });
    }
    return ar;
}

function setCardImageSource(image, thumb) {
    const source220 = thumb.urlCard220 || thumb.urlCard530 || thumb.urlA;
    const source530 = thumb.urlCard530 || source220;
    const sourceSet = source220 === source530
        ? source220
        : `${source220} 220w, ${source530} 530w`;
    image.sizes = '130px';
    image.srcset = sourceSet;
    image.src = source530;
}

function createGalleryPreview(image, modName, images) {
    const preview = document.createElement('div');
    preview.className = 'mod-gallery-preview';
    preview.appendChild(image);
    if (!Array.isArray(images) || images.length === 0) return preview;

    image.tabIndex = 0;
    image.setAttribute('role', 'button');
    image.setAttribute('aria-label', `Preview ${modName} (${images.length} image${images.length === 1 ? '' : 's'})`);
    image.onclick = () => openImageLightbox(modName, images);
    image.onkeydown = event => {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            image.click();
        }
    };
    if (images.length > 1) {
        const count = document.createElement('span');
        count.className = 'mod-gallery-count';
        count.innerText = `+${images.length - 1}`;
        count.setAttribute('aria-hidden', 'true');
        preview.appendChild(count);
    }
    return preview;
}

function openImageLightbox(modName, imageList, initialIndex = 0) {
    const dialog = document.getElementById('modImageLightbox');
    const image = document.getElementById('modImageLightboxImage');
    const viewport = document.getElementById('modImageViewport');
    const title = document.getElementById('modImageLightboxTitle');
    const counter = document.getElementById('modImageLightboxCounter');
    const thumbnails = document.getElementById('modImageLightboxThumbnails');
    const zoomLevel = document.getElementById('modImageZoomLevel');
    const previous = document.getElementById('modImagePrevious');
    const next = document.getElementById('modImageNext');
    const zoomOut = document.getElementById('modImageZoomOut');
    const zoomIn = document.getElementById('modImageZoomIn');
    const zoomReset = document.getElementById('modImageZoomReset');
    const close = document.getElementById('modImageLightboxClose');

    const images = Array.isArray(imageList)
        ? imageList.filter(item => typeof item?.urlA === 'string' && item.urlA.length > 0)
        : [];
    if (!dialog || !image || images.length === 0) return;

    let currentIndex = Math.min(Math.max(Number(initialIndex) || 0, 0), images.length - 1);
    let zoom = 1;

    const applyZoom = () => {
        const percent = Math.round(zoom * 100);
        zoomLevel.value = `${percent}%`;
        zoomLevel.textContent = `${percent}%`;
        zoomOut.disabled = zoom <= 1;
        zoomIn.disabled = zoom >= 3;
        image.style.width = zoom === 1 ? 'auto' : `${percent}%`;
        image.style.maxWidth = zoom === 1 ? '100%' : 'none';
        image.style.maxHeight = zoom === 1 ? '100%' : 'none';
        image.style.cursor = zoom >= 3 ? 'zoom-out' : 'zoom-in';
        viewport.scrollTo({ top: 0, left: 0 });
    };

    const setZoom = value => {
        zoom = Math.min(3, Math.max(1, value));
        applyZoom();
    };

    const renderImage = () => {
        const selected = images[currentIndex];
        zoom = 1;
        image.src = selected.urlA;
        image.alt = `${modName || 'Mod'} preview ${currentIndex + 1}`;
        title.textContent = modName || 'Image preview';
        counter.textContent = `${currentIndex + 1} of ${images.length}`;
        previous.disabled = images.length < 2;
        next.disabled = images.length < 2;

        thumbnails.replaceChildren();
        images.forEach((item, index) => {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'mod-image-lightbox-thumbnail';
            button.setAttribute('aria-label', `Show image ${index + 1}`);
            button.setAttribute('aria-current', index === currentIndex ? 'true' : 'false');
            const thumbnail = document.createElement('img');
            thumbnail.src = item.urlB || item.urlA;
            thumbnail.alt = '';
            thumbnail.onerror = () => {
                thumbnail.onerror = null;
                thumbnail.src = './img/mod-placeholder.png';
            };
            button.appendChild(thumbnail);
            button.onclick = () => {
                currentIndex = index;
                renderImage();
            };
            thumbnails.appendChild(button);
        });
        applyZoom();
    };

    const move = amount => {
        if (images.length < 2) return;
        currentIndex = (currentIndex + amount + images.length) % images.length;
        renderImage();
    };

    previous.onclick = () => move(-1);
    next.onclick = () => move(1);
    zoomOut.onclick = () => setZoom(zoom - 0.25);
    zoomIn.onclick = () => setZoom(zoom + 0.25);
    zoomReset.onclick = () => setZoom(1);
    close.onclick = () => dialog.close();
    image.onclick = () => setZoom(zoom >= 3 ? 1 : zoom + 0.25);
    image.onerror = () => {
        image.onerror = null;
        image.src = './img/mod-placeholder.png';
    };
    dialog.onclick = event => {
        if (event.target === dialog) dialog.close();
    };
    dialog.onkeydown = event => {
        if (event.key === 'ArrowLeft') {
            event.preventDefault();
            move(-1);
        } else if (event.key === 'ArrowRight') {
            event.preventDefault();
            move(1);
        } else if (event.key === '+' || event.key === '=') {
            event.preventDefault();
            setZoom(zoom + 0.25);
        } else if (event.key === '-') {
            event.preventDefault();
            setZoom(zoom - 0.25);
        } else if (event.key === '0') {
            event.preventDefault();
            setZoom(1);
        }
    };

    renderImage();
    if (!dialog.open) dialog.showModal();
    close.focus();
}

function currentContentFilter() {
    return localStorage.getItem('gamebananaContentFilter') || 'all';
}

function applyContentFilter(records) {
    const filter = currentContentFilter();
    const compatibleRecords = records.filter(mod => {
        if (mod._sModelName == 'Wip' && !mod._bHasFiles) return false;
        return mod._sModelName === 'Wip' || mod._sModelName === 'Mod';
    });
    const visibleRecords = compatibleRecords.filter(mod => {
        if (filter === 'unrated') return !mod._bHasContentRatings;
        if (filter === 'rated') return Boolean(mod._bHasContentRatings);
        return true;
    });
    const excluded = compatibleRecords.length - visibleRecords.length;
    const status = document.getElementById('contentFilterStatus');
    if (status) {
        status.innerText = filter === 'all'
            ? `Showing ${visibleRecords.length} compatible mod${visibleRecords.length === 1 ? '' : 's'} on this page.`
            : `Showing ${visibleRecords.length}; ${excluded} excluded by the visible Content filter.`;
    }
    return visibleRecords;
}

const featuredPeriodPriority = new Map([
    ['alltime', 7],
    ['year', 6],
    ['6month', 5],
    ['3month', 4],
    ['month', 3],
    ['week', 2],
    ['today', 1]
]);

function prioritizeFeaturedRecords(records, featuredIDs) {
    const bestRankByID = new Map();
    for (const featured of featuredIDs) {
        const rank = featuredPeriodPriority.get(featured.period) || 0;
        bestRankByID.set(featured.id, Math.max(bestRankByID.get(featured.id) || 0, rank));
    }

    return records
        .map((mod, index) => ({
            mod,
            index,
            rank: bestRankByID.get(mod._idRow) || 0
        }))
        .sort((left, right) => right.rank - left.rank || left.index - right.index)
        .map(entry => entry.mod);
}

function featuredRankForID(featuredIDs, id) {
    return featuredIDs.reduce((bestRank, featured) => {
        if (featured.id !== id) return bestRank;
        return Math.max(bestRank, featuredPeriodPriority.get(featured.period) || 0);
    }, 0);
}

function gameBananaRecordKey(record) {
    return `${record?._sModelName || 'Submission'}:${record?._idRow || 0}`;
}

async function getGameBananaFeaturedRecords(gameID) {
    if (!gameBananaFeaturedRecordsPromise) {
        gameBananaFeaturedRecordsPromise = browseGameBananaCatalog(
            `https://gamebanana.com/apiv11/Game/${gameID}/TopSubs`
        ).then(result => {
            const records = result.payload;
            return Array.isArray(records) ? records : [];
        }).catch(error => {
            console.warn('Featured GameBanana metadata is unavailable:', error);
            return [];
        });
    }
    return gameBananaFeaturedRecordsPromise;
}

function insertRankedGameBananaRow(table, row, rank) {
    row.dataset.featuredRank = String(rank);
    const lowerRankRow = Array.from(table.children).find(existingRow =>
        existingRow.dataset.featuredRank !== undefined &&
        Number(existingRow.dataset.featuredRank) < rank
    );
    if (lowerRankRow) table.insertBefore(row, lowerRankRow);
    else table.appendChild(row);
}

async function describeContentRatings(mod, chip) {
    if (!mod._bHasContentRatings) return;
    chip.innerText = 'Content-rated submission';
    try {
        const response = await fetch(`https://gamebanana.com/apiv11/${mod._sModelName}/${mod._idRow}/ProfilePage`);
        if (!response.ok) return;
        const profile = await response.json();
        const ratings = Object.values(profile._aContentRatings || {});
        if (ratings.length) chip.innerText = `Content: ${ratings.join(', ')}`;
    } catch {}
}
    
var isGBLoggedIn = false;

async function gameBananaLogin() {
    var loggedin = await Promise.race([
        window.deltamodBackend.invoke('validateGamebananaToken', []),
        new Promise(resolve => setTimeout(() => resolve(false), 5000))
    ]).catch(error => {
        console.warn('GameBanana account status is unavailable; continuing anonymously.', error);
        return false;
    });

    isGBLoggedIn = loggedin;

    const accountPicture = document.getElementById('gbPic');
    accountPicture.onclick = async () => {
        window._pageArguments = {
            cat: 'gb'
        };
        page('options');
    };

    if (loggedin) {
        var pic = await window.deltamodBackend.invokeOptional('getGamebananaPic', [], null);
        if (typeof pic === 'string' && pic.trim()) {
            accountPicture.src = pic;
            accountPicture.hidden = false;
            accountPicture.onerror = () => {
                accountPicture.hidden = true;
                accountPicture.removeAttribute('src');
            };
            return;
        }
    }
    accountPicture.hidden = true;
    accountPicture.removeAttribute('src');
};

function roundViews(views) {
    const n = Number(views) || 0;
    if (n >= 1000) return Math.round(n / 1000) + 'k';
    return String(n);
}

let capi = '';
let csearch = '';
function syncSearchClearButton() {
    const input = document.getElementById('searchInput');
    const clearButton = document.getElementById('clearModSearchButton');
    if (input && clearButton) clearButton.hidden = input.value.length === 0;
}

function hideSearchSuggestions() {
    const suggestions = document.querySelector('.autocomplete .results');
    if (!suggestions) return;
    suggestions.innerHTML = '';
    suggestions.style.opacity = '0';
    suggestions.style.pointerEvents = 'none';
}

function clearModSearch() {
    const input = document.getElementById('searchInput');
    if (input) input.value = '';
    csearch = '';
    syncSearchClearButton();
    hideSearchSuggestions();
    window._pageArguments = preserveShopArguments();
    page('gamebanana-browse');
}

const SHOP_ICON_PATHS = Object.freeze({
    download: '<path d="M12 4v10m-4-4 4 4 4-4M5 19h14"/>',
    loading: '<path d="M19.4 7.5A8 8 0 1 0 20 12"/>',
    cancel: '<path d="m6 6 12 12M18 6 6 18"/>',
    done: '<path d="m5 12 4 4L19 6"/>',
    question: '<circle cx="12" cy="12" r="8"/><path d="M9.8 9a2.4 2.4 0 0 1 4.6 1c0 1.8-2.4 2-2.4 4"/><path d="M12 17h.01"/>',
    comment: '<path d="M5 5h14v11H9l-4 3V5Z"/>',
    heart: '<path d="M20.5 9c0 4.8-8.5 10-8.5 10S3.5 13.8 3.5 9A4.5 4.5 0 0 1 12 7a4.5 4.5 0 0 1 8.5 2Z"/>',
    smile: '<circle cx="12" cy="12" r="8"/><path d="M9 10h.01M15 10h.01M8.5 14c1 1.4 2.1 2 3.5 2s2.5-.6 3.5-2"/>',
    open: '<path d="M13 5h6v6m0-6-9 9"/><path d="M17 13v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1h5"/>'
});

function shopIcon(name) {
    const aliases = {
        search_activity: 'loading',
        downloading: 'loading',
        progress_activity: 'loading',
        done_outline: 'done',
        indeterminate_question_box: 'question',
        mood_heart: 'heart',
        sentiment_very_satisfied: 'smile',
        open_in_new: 'open'
    };
    const iconName = aliases[name] || name;
    const path = SHOP_ICON_PATHS[iconName];
    if (!path) return icon(name, '0.9em');
    const spinning = iconName === 'loading' ? ' is-spinning' : '';
    return `<svg class="shop-action-icon${spinning}" viewBox="0 0 24 24" aria-hidden="true" focusable="false">${path}</svg>`;
}

function formatDownloadBytes(value) {
    const bytes = Math.max(0, Number(value) || 0);
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    const amount = bytes / (1024 ** unit);
    const decimals = unit === 0 || amount >= 10 || Number.isInteger(amount) ? 0 : 1;
    return `${amount.toFixed(decimals)} ${units[unit]}`;
}

function updateModDownloadStatus({ phase = 'download', completed = 0, total = 0, currentItem = '' }) {
    const panel = document.getElementById('modDownloadStatus');
    if (!panel) return;
    const title = document.getElementById('modDownloadStatusTitle');
    const percent = document.getElementById('modDownloadStatusPercent');
    const item = document.getElementById('modDownloadStatusItem');
    const bytes = document.getElementById('modDownloadStatusBytes');
    const track = document.getElementById('modDownloadProgressTrack');
    const bar = document.getElementById('modDownloadProgressBar');
    const completedBytes = Math.max(0, Number(completed) || 0);
    const totalBytes = Math.max(0, Number(total) || 0);
    const percentage = phase === 'complete'
        ? 100
        : totalBytes > 0
            ? Math.max(0, Math.min(100, (completedBytes / totalBytes) * 100))
            : 0;
    const titles = {
        download: 'Downloading mod…',
        import: 'Importing mod…',
        complete: 'Mod imported successfully',
        manual: 'Website confirmation required',
        failed: 'Download or import failed'
    };

    panel.hidden = false;
    panel.dataset.phase = phase;
    title.textContent = titles[phase] || titles.download;
    percent.textContent = phase === 'import'
        ? 'Importing'
        : phase === 'manual'
            ? 'Website'
        : phase === 'failed' && percentage === 0
            ? 'Failed'
            : `${Math.round(percentage)}%`;
    item.textContent = currentItem || 'Mod package';
    bytes.textContent = totalBytes > 0
        ? `${formatDownloadBytes(completedBytes)} / ${formatDownloadBytes(totalBytes)}`
        : completedBytes > 0
            ? formatDownloadBytes(completedBytes)
            : '';
    track.setAttribute('aria-valuenow', String(Math.round(percentage)));
    track.setAttribute('aria-valuetext', title.textContent);
    bar.style.setProperty('--mod-download-progress', `${percentage}%`);
}

function setDownloadButtonIcon(button, glyph) {
    button.innerHTML = shopIcon(glyph);
}

window.currentPageStack.updateModDownloadStatus = updateModDownloadStatus;

async function search(searchQuery = null) {
    const input = document.getElementById('searchInput');
    let query = String(searchQuery ?? input.value).trim();
    if (searchQuery !== null) {
        input.value = String(searchQuery);
    }
    syncSearchClearButton();
    if (query.length === 0) {
        clearModSearch();
        return;
    }
    if (query.length < 2) {
        await htmlAlert("Search query too short","Please enter at least 2 characters to search.",[{text:"Ok",resolveWith:'ok'}], 'error');
        return;
    }

    if (SHOP_PROVIDER !== 'gamebanana') {
        window._pageArguments = preserveShopArguments({
            sourceQuery: query.trim()
        });
        page('gamebanana-browse');
        return;
    }

    const gameID = currentShopGameBananaId();
    if (!gameID) {
        await htmlAlert(
            'GameBanana unavailable for this game',
            'Choose a game with GameBanana support from the Mod Shop game selector.',
            [{ text: 'OK', resolveWith: 'ok' }],
            'error'
        );
        return;
    }

    {
        // Search names, descriptions, owners, credits, and studios in one
        // request so creator names work without a separate author-only mode.
        window._pageArguments.gbAPI = 'https://gamebanana.com/apiv11/Util/Search/Results?_sModelName=Mod&_sOrder=best_match&_sSearchString=' + encodeURIComponent(query) + '&_csvFields=name%2Cdescription%2Carticle%2Cattribs%2Cstudio%2Cowner%2Ccredits&_idGameRow=' + gameID + '&_nPage=$PAGE';
        window._pageArguments.gbAPIFilter = async function(data) {
            return data;
        };
    }
    window._pageArguments.leSearchQuery = query;
    page('gamebanana-browse');
}

async function featured() {
    const gameID = currentShopGameBananaId();
    if (!gameID) return;
    // Why doesn't GB have a standard endpoint format for subs SMH
    window._pageArguments.gbAPI = 'https://gamebanana.com/apiv11/Game/' + gameID + '/TopSubs';
    window._pageArguments.gbAPIFilter = async function(data) {
        return {
            _aMetadata: { _bIsComplete: true },
            _aRecords: data.map(x => {
                x.featuredDataset = true;
                return x;
            })
        };
    } 
    page('gamebanana-browse');
}

window.currentPageStack.featured = featured;
window.currentPageStack.qms = {}; //queryme stack

async function dlmod(dlurl, buttonElem=null, modid, modmodel, currentItem = `GameBanana ${modmodel} ${modid}`) {
    if (!isCurrentShopPage() || gameBananaDownloadActive) return false;
    gameBananaDownloadActive = true;
    const sidebarButtons = Array.from(document.querySelectorAll('.sidebar-button'))
        .filter(button => !button.disabled);
    sidebarButtons.forEach(button => { button.disabled = true; });
    const pageCleanups = window._onClosePage;
    let navigationReleased = false;
    const releaseNavigation = () => {
        if (navigationReleased) return;
        navigationReleased = true;
        sidebarButtons.forEach(button => { button.disabled = false; });
        // Do not mutate the cleanup array while navigation is iterating it.
        if (pageActive) {
            const index = pageCleanups.indexOf(releaseNavigation);
            if (index !== -1) pageCleanups.splice(index, 1);
        }
    };
    pageCleanups.push(releaseNavigation);
    let queryme = Math.random().toString(36).substring(2, 15);

    setDownloadButtonIcon(buttonElem, 'search_activity');
    updateModDownloadStatus({ phase: 'download', currentItem });

    const progressHandlers = window.currentPageStack.qms;
    let imported = false;
    if (buttonElem) {
        buttonElem.disabled = true;
        buttonElem.setAttribute('aria-busy', 'true');
    }
    progressHandlers[queryme] = function(info) {
        if (!isCurrentShopPage()) return;
        if (info.error) {
            setDownloadButtonIcon(buttonElem, 'cancel');
            updateModDownloadStatus({
                phase: 'failed',
                currentItem
            });
            return;
        }

        const p = Math.max(0, Math.min(100, Number(info.progress) || 0));
        updateModDownloadStatus({
            phase: info.phase || 'download',
            completed: info.downloaded,
            total: info.total,
            currentItem
        });
        buttonElem.style.transition = 'none';
        buttonElem.classList.add('download-progress');
        buttonElem.style.setProperty('--download-progress', `${p}%`);

    };

    try {
        await window.deltamodBackend.invoke('dlmodURL',[dlurl, queryme, modid, modmodel]);
        imported = true;
        if (!isCurrentShopPage()) return true;
        setDownloadButtonIcon(buttonElem, 'done_outline');
        updateModDownloadStatus({ phase: 'complete', currentItem });
        return true;
    } catch (error) {
        if (!isCurrentShopPage()) return false;
        setDownloadButtonIcon(buttonElem, 'cancel');
        updateModDownloadStatus({ phase: 'failed', currentItem });
        await htmlAlert(
            'Download failed',
            error?.message || 'The mod could not be downloaded or imported.',
            [{ text: 'OK', resolveWith: 'ok' }]
        );
        return false;
    } finally {
        delete progressHandlers[queryme];
        gameBananaDownloadActive = false;
        if (buttonElem) {
            buttonElem.disabled = imported;
            buttonElem.removeAttribute('aria-busy');
            buttonElem.style.opacity = '';
            buttonElem.classList.remove('download-progress');
            buttonElem.style.removeProperty('--download-progress');
            if (!imported) setDownloadButtonIcon(buttonElem, 'download');
        }
        releaseNavigation();
    }
}

window.currentPageStack.dlmod = async function(info) {
    let queryme = info.queryme;
    let qms = window.currentPageStack.qms;
    if (!qms[queryme]) {
        console.warn('Received dlmod progress for unknown queryme: ' + queryme);
        return;
    }
    let qme = qms[queryme];
    qme(info);
} 

window.currentPageStack.search = search;

window.currentPageStack.plusPage = plusPage;

window.currentPageStack.openImageLightbox = openImageLightbox;

var firstgeneration = true;

async function fetchGameBananaCatalogDirect(url) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 12000);
    try {
        const response = await fetch(url, { signal: controller.signal });
        if (!response.ok) {
            const error = new Error(`GameBanana returned HTTP ${response.status}.`);
            error.status = response.status;
            throw error;
        }
        return {
            provider: 'gamebanana',
            payload: await response.json(),
            direct: true
        };
    } catch (error) {
        if (error?.name === 'AbortError') {
            const timeoutError = new Error('GameBanana did not respond before the request timed out.');
            timeoutError.code = 'GAMEBANANA_TIMEOUT';
            throw timeoutError;
        }
        throw error;
    } finally {
        clearTimeout(timeout);
    }
}

async function browseGameBananaCatalog(url) {
    let nativeFailure = null;
    try {
        const nativeRequest = requestExternalSource({
            provider: 'gamebanana',
            url,
            gameId: SHOP_GAME_ID
        });
        const response = await Promise.race([
            nativeRequest,
            new Promise((_, reject) => setTimeout(() => {
                const error = new Error('The native GameBanana request timed out.');
                error.code = 'GAMEBANANA_NATIVE_TIMEOUT';
                reject(error);
            }, 9000))
        ]);
        if (response?.ok && response.result?.payload !== undefined) {
            return response.result;
        }
        nativeFailure = new Error(
            response?.error?.message || 'The native GameBanana catalogue request failed.'
        );
        nativeFailure.code = response?.error?.code || 'provider_unavailable';
    } catch (error) {
        nativeFailure = error;
    }

    // Electron intentionally leaves GameBanana on its compatibility renderer
    // path. This fallback also keeps Linux WebKit usable if the native bridge
    // is unavailable while the public GameBanana endpoint itself is reachable.
    try {
        return await fetchGameBananaCatalogDirect(url);
    } catch (directFailure) {
        if (nativeFailure && !directFailure.cause) directFailure.cause = nativeFailure;
        throw directFailure;
    }
}

async function renderMods(table, GB_API, filter, gameID) {
    if (!isCurrentShopPage() || typeof GB_API !== 'string' || !table?.isConnected) {
        return false;
    }
    if (window.PAGE == null) {
        window.PAGE = 1;
    }
    var furl = GB_API.replace('$PAGE', window.PAGE);
    const requestedPage = window.PAGE;
    let catalog;
    let data;
    let featuredData;
    try {
        catalog = await browseGameBananaCatalog(furl);
        if (!isCurrentShopPage()) return;
        data = await filter(catalog.payload);
        if (!isCurrentShopPage()) return;
        featuredData = await getGameBananaFeaturedRecords(gameID);
        if (!isCurrentShopPage()) return;
    } catch (error) {
        console.error('GameBanana catalogue load failed:', error);
        if (isCurrentShopPage()) {
            renderGameBananaFailure(
                table,
                'GameBanana could not be loaded',
                error?.message || 'The catalogue request failed. Check your connection and try again.',
                requestedPage
            );
        }
        return false;
    }

    const status = document.getElementById('contentFilterStatus');
    if (catalog.stale) {
        status.innerText = 'Showing saved GameBanana results while the live catalogue is unavailable.';
    } else if (catalog.cached) {
        status.innerText = 'Loaded this GameBanana page from the local catalogue cache.';
    }

    var featuredIDs = featuredData.map(x => {return {id: x._idRow, period: x._sPeriod};});

    if (firstgeneration) {
        table.replaceChildren();
        renderedGameBananaRecords.clear();
    }

    try {
        const complete = Boolean(data._aMetadata._bIsComplete);
        const pageRecords = Array.isArray(data._aRecords) ? data._aRecords : [];
        const isFeaturedDataset = pageRecords.some(record => record.featuredDataset);
        const candidates = firstgeneration && !isFeaturedDataset && /\/Subfeed(?:\?|$)/.test(GB_API)
            ? [...featuredData, ...pageRecords]
            : pageRecords;
        const pageKeys = new Set();
        const pageRows = [];
        const records = prioritizeFeaturedRecords(applyContentFilter(candidates), featuredIDs)
            .filter(record => {
                const key = gameBananaRecordKey(record);
                if (renderedGameBananaRecords.has(key) || pageKeys.has(key)) return false;
                pageKeys.add(key);
                return true;
            });

        if (records.length == 0 && firstgeneration) {
            var tr = document.createElement('tr');
            var td = document.createElement('td');
            td.colSpan = 2;
            td.innerText = currentContentFilter() === 'all'
                ? "No mods were found matching your query."
                : "No submissions on this page match the active Content filter.";
            tr.appendChild(td);
            table.appendChild(tr);
            gameBananaHasMore = false;
            firstgeneration = false;
            observer.disconnect(); // stop observing since there's no more content to load
            document.querySelector('.scrollBottomDetector').style.display = 'none'; // hide the loading indicator
            return true;
        }
        for (const mod of records) {
            await (async () => {
                var tr = document.createElement('tr');
                tr.dataset.canonicalIdentity = `gamebanana:${mod._sModelName || 'Submission'}:${mod._idRow || 0}`;

                var td0 = document.createElement('td');
                td0.style.display = 'flex';
                td0.style.alignItems = 'center';
                td0.style.gap = '14px';
                td0.style.justifyContent = 'left';
                // Rendering of td0
                {
                let thumbs = getAllThumbs(mod);
                var img = document.createElement('img');
                img.className = 'modThumbImg';
                setCardImageSource(img, thumbs[0]);
                img.loading = 'lazy';
                img.decoding = 'async';
                img.alt = `${mod._sName || 'Mod'} preview`;
                img.onerror = () => {
                    img.onerror = null;
                    img.src = './img/mod-placeholder.png';
                };
                img.style.width = '130px';
                img.style.aspectRatio = '16 / 9';
                img.style.height = 'auto';
                img.style.objectFit = 'cover';
                img.style.objectPosition = 'center';

                var div0 = createGalleryPreview(img, mod._sName, thumbs);
                div0.classList.add('modThumbDiv');

                var div1 = document.createElement('div');
                div1.className = 'modCopy external-source-card';
                td0.appendChild(div0);
                td0.appendChild(div1);

                var biggerSpan = document.createElement('span');
                biggerSpan.className = 'modTitleSpan';
                biggerSpan.style.fontSize = '1.2em';
                biggerSpan.style.marginBottom = '0px';
                biggerSpan.innerText = mod._sName;
                biggerSpan.style.cursor = 'pointer';
                biggerSpan.onclick = () => {
                    window.open(mod._sProfileUrl, '_blank');
                };
                div1.appendChild(biggerSpan);
                var sourceBadge = document.createElement('span');
                sourceBadge.className = 'external-source-badge';
                sourceBadge.innerText = 'GameBanana';
                div1.appendChild(sourceBadge);

                var otherInfoSpan = document.createElement('div');
                otherInfoSpan.className = 'modOtherInfoSpan calibri';
                otherInfoSpan.style.fontSize = '0.9em';
                otherInfoSpan.style.display = 'flex';
                otherInfoSpan.style.alignItems = 'center';
                otherInfoSpan.style.flexWrap = 'wrap';
                otherInfoSpan.style.gap = '8px';
                otherInfoSpan.style.color = '#cccccc';
                otherInfoSpan.style.marginTop = '7px';
                otherInfoSpan.style.width = '100%';

                var nameauthor = mod._aSubmitter._sName;
                // easter egg for the tenna lover
                if (mod._aSubmitter._idRow == 1712567) {
                    nameauthor += ' (Tenna lover)';
                }
                var authorSpan = document.createElement('span');
                authorSpan.className = 'modAuthorSpan iptspan';
                authorSpan.style.marginRight = '12px';
                var authorImage = document.createElement('img');
                authorImage.src = mod._aSubmitter._sAvatarUrl || './img/mod-placeholder.png';
                authorImage.alt = '';
                authorImage.loading = 'lazy';
                authorImage.decoding = 'async';
                authorImage.className = 'modAvatarImg';
                authorImage.onerror = () => {
                    authorImage.onerror = null;
                    authorImage.src = './img/mod-placeholder.png';
                };
                authorSpan.append(authorImage, document.createTextNode(nameauthor));
                authorSpan.onclick = () => {
                    window.open(mod._aSubmitter._sProfileUrl, '_blank');
                };
                authorSpan.style.cursor = 'pointer';

                var e = null;
                if (featuredIDs.find(x => x.id === mod._idRow)) {
                    biggerSpan.style.color = 'gold';
                    img.style.borderColor = 'gold';

                    var periodsDesc = [
                        ["alltime","All-time featured"],
                        ["year","Best of this year"],
                        ["6month","Best of last 6 months"],
                        ["3month","Best of last 3 months"],
                        ["month","Best of this month"],
                        ["week","Best of this week"],
                        ["today","Best of today"]
                    ]
                    var featSpan = document.createElement('span');
                    featSpan.className = 'modFeaturedSpan iptspan';
                    featSpan.style.display = 'inline-block';
                    for (let pd of periodsDesc) {
                    if (featuredIDs.find(x => x.id === mod._idRow && x.period === pd[0])) {
                        featSpan.innerHTML = `${icon((pd[0] == 'alltime' ? "award_star" : "editor_choice"),'1.1em')} ${pd[1]}`;
                        break;
                    }
                    }
                    featSpan.style.color = 'gold';
                    featSpan.style.marginRight = '12px';
                    e = featSpan;
                }
                otherInfoSpan.appendChild(authorSpan);
                if (e) otherInfoSpan.appendChild(e);
                if (mod._bHasContentRatings) {
                    const ratingChip = document.createElement('span');
                    ratingChip.className = 'content-rating-chip';
                    ratingChip.setAttribute('role', 'note');
                    otherInfoSpan.appendChild(ratingChip);
                    describeContentRatings(mod, ratingChip);
                }
                div1.appendChild(otherInfoSpan);

                var addDate = mod._tsDateAdded || 0;
                var modDate = mod._tsDateModified || 0;
                const timestamp = Math.max(addDate, modDate);

                if (timestamp > 0) {
                    var date = new Date(timestamp * 1000);
                    var desc = document.createElement('span');
                    desc.className = 'modDescSpan iptspan';
                    const relativeDate = (() => {
                    const diffSeconds = Math.round((date.getTime() - Date.now()) / 1000);
                    const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
                    /** @type {Array<{limit:number,value:number,unit:Intl.RelativeTimeFormatUnit}>} */
                    const units = [
                        { limit: 60, value: 1, unit: 'second' },
                        { limit: 3600, value: 60, unit: 'minute' },
                        { limit: 86400, value: 3600, unit: 'hour' },
                        { limit: 2592000, value: 86400, unit: 'day' },
                        { limit: 31536000, value: 2592000, unit: 'month' },
                        { limit: Infinity, value: 31536000, unit: 'year' }
                    ];

                    for (const { limit, value, unit } of units) {
                        if (Math.abs(diffSeconds) < limit) {
                            return rtf.format(Math.round(diffSeconds / value), /** @type {Intl.RelativeTimeFormatUnit} */ (unit));
                        }
                    }
                    })();

                    desc.innerHTML = icon('acute', '1.1em') + ' ' + relativeDate;
                    otherInfoSpan.appendChild(desc);
                }

                var summary = document.createElement('div');
                summary.className = 'external-source-summary calibri';
                summary.innerText = mod._sDescription || 'No description was provided.';
                div1.appendChild(summary);
                }

                var td1 = document.createElement('td');
                td1.style.textAlign = 'center';
                // Rendering of td1
                {
                    var dlBtn = document.createElement('button');
                    dlBtn.innerHTML = shopIcon('download');
                    dlBtn.className = 'serietast';
                    dlBtn.title = 'Download and import mod';
                    dlBtn.setAttribute('aria-label', `Download ${mod._sName}`);
                    dlBtn.onclick = async () => {
                        if (!isCurrentShopPage() || gameBananaDownloadActive || dlBtn.disabled) return;
                        dlBtn.disabled = true;
                        dlBtn.setAttribute('aria-busy', 'true');
                        setDownloadButtonIcon(dlBtn, 'downloading');
                        dlBtn.style.opacity = '0.7';

                        const resetDownloadButton = () => {
                            dlBtn.disabled = false;
                            dlBtn.removeAttribute('aria-busy');
                            dlBtn.style.opacity = '';
                            setDownloadButtonIcon(dlBtn, 'download');
                        };
                        let dlpage;
                        try {
                            const response = await fetchGameBananaCatalogDirect(`https://gamebanana.com/apiv11/${mod._sModelName}/${mod._idRow}/ProfilePage`);
                            dlpage = response.payload;
                            if (!Array.isArray(dlpage?._aFiles)) throw new Error('GameBanana returned an invalid file list.');
                        } catch (error) {
                            resetDownloadButton();
                            if (isCurrentShopPage()) {
                                await htmlAlert('Download failed', error?.message || 'The file list could not be loaded. Try again.', [{ text: 'OK', resolveWith: 'ok' }]);
                            }
                            return;
                        }
                        if (!isCurrentShopPage() || !dlBtn.isConnected) return;
                        if (gameBananaDownloadActive) {
                            resetDownloadButton();
                            return;
                        }
                        dlBtn.removeAttribute('aria-busy');

                        var eligibleDownloads = [];

                        dlpage._aFiles.forEach(file => {
                            try {
                                var mmo = file._aModManagerIntegrations.map(x => x._idToolRow);
                                if (mmo.includes(20575) && typeof file._sDownloadUrl === 'string' && file._sDownloadUrl.trim()) {
                                    eligibleDownloads.push(file);
                                }
                            }
                            catch {
                                //nothing, file is just not compatible
                            }
                        });

                        if (eligibleDownloads.length === 0) {
                            setDownloadButtonIcon(dlBtn, 'cancel');
                            var open = await htmlAlert("One-click download not available", "This mod cannot be downloaded via Deltamod because the owner did not package it for usage with the tool.",[{text:"Ok",resolveWith:'no',},{text:"Open mod page on GameBanana",resolveWith:'yes'}], 'web_traffic');
                            if (open === 'yes') {
                                window.open(mod._sProfileUrl, '_blank');
                            }
                            resetDownloadButton();
                            return;
                        }

                        if (eligibleDownloads.length > 1) {
                            setDownloadButtonIcon(dlBtn, 'indeterminate_question_box');
                            var dtr = document.createElement('tr');
                            var td = document.createElement('td');
                            td.colSpan = 2;
                            dtr.appendChild(td);
                            
                            var btnsDiv = document.createElement('div');
                            td.appendChild(btnsDiv);

                            eligibleDownloads.forEach((file) => {
                                console.log(JSON.stringify(file));
                                var thisBtn = document.createElement('button');
                                thisBtn.style.display = 'inline-flex';
                                thisBtn.style.alignItems = 'center';
                                thisBtn.style.gap = '4px';
                                thisBtn.style.margin = '4px';
                                thisBtn.style.width = '100%';
                                thisBtn.onclick = async () => {
                                    if (!dtr.isConnected || gameBananaDownloadActive) return;
                                    dtr.remove();
                                    await dlmod(file._sDownloadUrl.replace('dl','mmdl'), dlBtn, mod._idRow, mod._sModelName, file._sFile || file._sName || mod._sName);
                                };
                                btnsDiv.appendChild(thisBtn);

                                var dlIcon = document.createElement('span');
                                dlIcon.innerHTML = icon('download', '1.1em');
                                thisBtn.appendChild(dlIcon);

                                var details = document.createElement('div');
                                details.style.textAlign = 'left';
                                thisBtn.appendChild(details);

                                var filename = document.createElement('span');
                                filename.innerText = file._sFile;
                                filename.style.display = 'block';
                                filename.style.fontWeight = 'bold';
                                filename.style.fontSize = '1.1em';
                                details.appendChild(filename);

                                var filesize = document.createElement('span');
                                filesize.style.display = 'block';
                                filesize.innerText = ` (${(file._nFilesize / 1024 / 1024).toFixed(2)} MB)`;
                                filesize.style.fontSize = '0.9em';
                                details.appendChild(filesize);

                                var filedesc = document.createElement('span');
                                filedesc.style.display = 'block';
                                filedesc.innerText = file._sDescription || "No description provided.";
                                filedesc.style.fontSize = '0.9em';
                                filedesc.style.color = '#ffffff60';
                                details.appendChild(filedesc);

                                var filedate = document.createElement('span');
                                var fdate = new Date(file._tsDateAdded * 1000);
                                filedate.style.display = 'block';
                                filedate.innerText = `Added on ${fdate.toLocaleDateString()} at ${fdate.toLocaleTimeString()}`;
                                filedate.style.fontSize = '0.9em';
                                filedate.style.color = '#ffffff60';
                                details.appendChild(filedate);

                                if (file._sAvState == 'done' && file._sAvResult != 'clean') {
                                    var avspan = document.createElement('span');
                                    avspan.style.display = 'block';
                                    avspan.style.fontSize = '0.9em';
                                    avspan.style.color = '#ffffff';
                                    avspan.style.backgroundColor = '#980000';
                                    avspan.style.padding = '2px 4px';
                                    avspan.innerText = `File flagged: ${file._sAnalysisResultVerbose}`;
                                    details.appendChild(avspan);
                                }
                            });

                            const cancelChoice = document.createElement('button');
                            cancelChoice.type = 'button';
                            cancelChoice.dataset.i18n = 'cancel';
                            cancelChoice.textContent = window.Localization.t('cancel', 'Cancel');
                            cancelChoice.onclick = () => {
                                dtr.remove();
                                resetDownloadButton();
                                dlBtn.focus();
                            };
                            btnsDiv.appendChild(cancelChoice);
                            tr.insertAdjacentElement("afterend", dtr);
                            btnsDiv.querySelector('button')?.focus();

                            await timeoutPromise(100);
                            rew();

                            return;
                        }

                        const file = eligibleDownloads[0];
                        await dlmod(file._sDownloadUrl.replace('dl','mmdl'), dlBtn, mod._idRow, mod._sModelName, file._sFile || file._sName || mod._sName);
                    };

                    td1.appendChild(dlBtn);

                    var commentBtn = document.createElement('button');
                    commentBtn.innerHTML = shopIcon('comment');
                    commentBtn.style.marginLeft = '8px';
                    commentBtn.className = 'serietast';
                    commentBtn.title = 'View comments';
                    commentBtn.setAttribute('aria-label', `View comments for ${mod._sName}`);
                    commentBtn.onclick = async () => {
                        window._pageArguments = {
                            id: mod._idRow,
                            model: mod._sModelName
                        };
                        page('gamebanana-leave-comment');
                    };
                    td1.appendChild(commentBtn);

                    var likeBtn = document.createElement('button');
                    likeBtn.innerHTML = shopIcon('heart');
                    likeBtn.style.marginLeft = '8px';
                    likeBtn.className = 'serietast';
                    likeBtn.title = isGBLoggedIn ? 'Like mod' : 'Log in to GameBanana to like mods';
                    likeBtn.setAttribute('aria-label', `Like ${mod._sName}`);
                    likeBtn.disabled = !isGBLoggedIn;
                    likeBtn.onclick = async () => {
                        let res = await window.deltamodBackend.invoke('gbLikeMod',[mod._sModelName, mod._idRow]);
                        if (res.status == 200) {
                            setDownloadButtonIcon(likeBtn, 'sentiment_very_satisfied');
                            likeBtn.disabled = true;
                        }
                        else if (res.data._sErrorCode.toLowerCase() == 'already_liked') {
                            await htmlAlert("Can't like the mod","You've already liked this mod. Can't get any more likes than that!",[{text:'Ok',resolveWith:'ok'}], 'sentiment_very_satisfied');
                            setDownloadButtonIcon(likeBtn, 'sentiment_very_satisfied');
                            likeBtn.disabled = true;
                        } else {
                            await htmlAlert("Can't like the mod",res.data._sErrorCode,[{text:'Ok',resolveWith:'ok'}], 'error');
                        }
                    };
                    td1.appendChild(likeBtn);
                }

                // tr-ify and add
                tr.appendChild(td0);
                tr.appendChild(td1);

                pageRows.push({ row: tr, rank: featuredRankForID(featuredIDs, mod._idRow) });
            })();
        };
        if (!isCurrentShopPage()) return false;
        // Publish a complete page together, so malformed later records cannot
        // leave partial rows or deduplication keys behind on a retry.
        for (const { row, rank } of pageRows) insertRankedGameBananaRow(table, row, rank);
        for (const key of pageKeys) renderedGameBananaRecords.add(key);
        if (complete) {
            gameBananaHasMore = false;
            observer.disconnect();
            document.querySelector('.scrollBottomDetector').style.display = 'none';
        }
    }
    catch (e) {
        console.error(e);
        if (isCurrentShopPage()) {
            renderGameBananaFailure(
                table,
                'GameBanana results could not be rendered',
                e?.message || 'The catalogue returned data that Community could not display.',
                requestedPage
            );
        }
        return false;
    }

    firstgeneration = false;
    return true;
}

function renderGameBananaFailure(table, title, message, requestedPage) {
    gameBananaRetryPending = true;
    const retry = () => loadGameBananaPage(requestedPage);
    if (firstgeneration) {
        renderSourceState(table, title, message, {
            label: window.Localization.t('refine_retry', 'Retry'), run: retry
        });
        return;
    }

    // Keep the pages already read in place. A failed page must be retried
    // explicitly before infinite scrolling can request the following page.
    table.querySelector('.shop-page-retry')?.remove();
    const row = document.createElement('tr');
    row.className = 'shop-page-retry';
    const cell = document.createElement('td');
    cell.colSpan = 2;
    const detail = document.createElement('p');
    detail.className = 'calibri';
    detail.setAttribute('role', 'status');
    detail.dataset.i18n = 'shop_next_page_failed';
    detail.textContent = window.Localization.t('shop_next_page_failed', 'The next page could not be loaded. Your current results are still here.');
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.i18n = 'refine_retry';
    button.textContent = window.Localization.t('refine_retry', 'Retry');
    button.onclick = retry;
    cell.append(detail, button);
    row.append(cell);
    table.append(row);
}

function renderSourceState(table, title, message, action = null) {
    table.closest('table')?.classList.add('is-state');
    table.replaceChildren();
    const tr = document.createElement('tr');
    const td = document.createElement('td');
    td.colSpan = 2;
    td.className = 'shop-state';
    const heading = document.createElement('h2');
    heading.innerText = title;
    const detail = document.createElement('p');
    detail.className = 'calibri';
    detail.innerText = message;
    td.append(heading, detail);
    if (action) {
        const button = document.createElement('button');
        button.type = 'button';
        button.innerText = action.label;
        button.onclick = action.run;
        td.appendChild(button);
    }
    tr.appendChild(td);
    table.appendChild(tr);
}

function renderSourceLoading(table, rowCount = 4) {
    table.closest('table')?.classList.remove('is-state');
    table.replaceChildren();

    for (let index = 0; index < rowCount; index += 1) {
        const row = document.createElement('tr');
        row.className = 'shop-skeleton-row';
        row.setAttribute('aria-hidden', 'true');

        const cell = document.createElement('td');
        cell.colSpan = 2;

        const skeleton = document.createElement('div');
        skeleton.className = 'shop-skeleton';

        const thumbnail = document.createElement('span');
        thumbnail.className = 'shop-skeleton-thumb';

        const copy = document.createElement('span');
        copy.className = 'shop-skeleton-copy';
        copy.append(document.createElement('i'), document.createElement('i'), document.createElement('i'));

        const actions = document.createElement('span');
        actions.className = 'shop-skeleton-actions';
        actions.append(document.createElement('i'), document.createElement('i'));

        skeleton.append(thumbnail, copy, actions);
        cell.appendChild(skeleton);
        row.appendChild(cell);
        table.appendChild(row);
    }
}

function formatSourceDate(value) {
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return 'Date unavailable';
    return new Intl.DateTimeFormat(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short'
    }).format(date);
}

function setExternalSourceControlsDisabled(disabled) {
    const searchInput = document.getElementById('searchInput');
    if (searchInput) searchInput.disabled = disabled;
    const searchButton = document.getElementById('modShopSearchButton');
    if (searchButton) searchButton.disabled = disabled;
    const clearButton = document.getElementById('clearModSearchButton');
    if (clearButton) clearButton.disabled = disabled;
    const sort = document.getElementById('nexusSort');
    if (sort) sort.disabled = disabled;
}

function externalBrowseRequestKey(request) {
    return JSON.stringify([
        request?.provider || '',
        request?.gameId || '',
        request?.query || '',
        request?.sort || '',
        request?.url || ''
    ]);
}

function requestExternalSource(request) {
    const key = externalBrowseRequestKey(request);
    const active = externalBrowseState.active;
    if (active) {
        if (active.key === key) return active.promise;
        // A new search may be requested from a freshly mounted page while the
        // old page is still waiting on the network. Queue it behind the old
        // request so only one Nexus call is active at a time.
        return active.promise
            .catch(() => undefined)
            .then(() => requestExternalSource(request));
    }

    const promise = Promise.resolve().then(() => window.communityAPI.modSources.browse(request));
    externalBrowseState.active = { key, promise };
    promise.finally(() => {
        if (externalBrowseState.active?.promise === promise) {
            externalBrowseState.active = null;
        }
    }).catch(() => {});
    return promise;
}

function copyRateLimitMetadata(target, source) {
    if (!source || typeof source !== 'object') return target;
    if (source.retryAfterMs != null && Number.isFinite(Number(source.retryAfterMs))) {
        target.retryAfterMs = Math.max(0, Number(source.retryAfterMs));
    }
    if (source.retryAt != null) target.retryAt = source.retryAt;
    if (source.quota && typeof source.quota === 'object') target.quota = source.quota;
    return target;
}

function nexusRateLimitMetadata(error) {
    const retryAtMs = Date.parse(String(error?.retryAt || ''));
    const retryAfterMs = Number(error?.retryAfterMs);
    let waitMs = Number.isFinite(retryAfterMs) ? Math.max(0, retryAfterMs) : 0;
    if (Number.isFinite(retryAtMs)) waitMs = Math.max(waitMs, retryAtMs - Date.now());
    if (!waitMs) {
        const quotaResets = ['daily', 'hourly']
            .map(period => Date.parse(String(error?.quota?.[period]?.resetAt || '')))
            .filter(Number.isFinite)
            .map(timestamp => timestamp - Date.now())
            .filter(value => value > 0);
        if (quotaResets.length) waitMs = Math.min(...quotaResets);
    }
    // A typed rate-limit response without a usable reset value still deserves
    // a quiet wait period; never offer an immediate retry that can hammer 429.
    if (!waitMs) waitMs = 60 * 1000;
    return {
        waitMs,
        retryAtMs: Number.isFinite(retryAtMs) ? retryAtMs : Date.now() + waitMs
    };
}

function formatRateLimitMessage(error) {
    const { waitMs, retryAtMs } = nexusRateLimitMetadata(error);
    const waitSeconds = Math.max(1, Math.ceil(waitMs / 1000));
    const waitText = waitSeconds < 120
        ? `${waitSeconds} second${waitSeconds === 1 ? '' : 's'}`
        : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
            .format(new Date(retryAtMs));
    const quotaParts = ['daily', 'hourly']
        .map(period => {
            const window = error?.quota?.[period];
            if (!window || !Number.isFinite(Number(window.remaining))) return '';
            const label = period === 'daily' ? 'today' : 'this hour';
            return `${window.remaining} ${label}`;
        })
        .filter(Boolean);
    const quotaText = quotaParts.length ? ` Remaining quota: ${quotaParts.join(', ')}.` : '';
    return `Nexus Mods asked Community to wait before another catalogue request. Try again after ${waitText}.${quotaText}`;
}

function scheduleNexusRateLimitRetry(table, error) {
    if (externalRetryTimer) clearTimeout(externalRetryTimer);
    const { waitMs } = nexusRateLimitMetadata(error);
    // setTimeout accepts a signed 32-bit delay; a long quota reset is still
    // represented in the message, while the controls remain disabled safely.
    const delay = Math.min(Math.max(1000, waitMs), 2_147_000_000);
    externalRetryTimer = setTimeout(() => {
        externalRetryTimer = null;
        if (!isCurrentShopPage()) return;
        setExternalSourceControlsDisabled(false);
        renderSourceState(
            table,
            'Nexus Mods ready to retry',
            'The reported quota window has elapsed. Retry when you are ready.',
            { label: 'Retry', run: () => initializeExternalSource(table) }
        );
    }, delay);
}

function isNexusRateLimited(error) {
    return SHOP_PROVIDER === 'nexus'
        && (error?.code === 'NEXUS_RATE_LIMITED' || Number(error?.status) === 429);
}

async function downloadNexusSource(item, button) {
    const operationId = crypto.randomUUID();
    const original = button.innerHTML;
    button.disabled = true;
    setDownloadButtonIcon(button, 'progress_activity');
    updateModDownloadStatus({ phase: 'download', currentItem: item.title });
    const unsubscribe = window.communityAPI.modSources.onProgress(progress => {
        if (progress.operationId !== operationId) return;
        updateModDownloadStatus({
            phase: progress.phase || 'download',
            completed: progress.completed,
            total: progress.total,
            currentItem: progress.currentItem || item.title
        });
        if (progress.total > 0) {
            const percentage = Math.max(0, Math.min(100, (progress.completed / progress.total) * 100));
            button.classList.add('download-progress');
            button.style.setProperty('--download-progress', `${percentage}%`);
        }
    });
    try {
        const response = await window.communityAPI.modSources.downloadNexus({
            modId: item.id,
            operationId,
            sourceUrl: item.sourceUrl
        });
        if (response?.ok === false) {
            const error = new Error(response?.error?.message || 'Nexus Mods download is unavailable.');
            error.code = response?.error?.code || 'NEXUS_DOWNLOAD_FAILED';
            if (Number.isInteger(response?.error?.status)) error.status = response.error.status;
            copyRateLimitMetadata(error, response?.error);
            throw error;
        }
        setDownloadButtonIcon(button, 'done_outline');
        updateModDownloadStatus({ phase: 'complete', currentItem: item.title });
    } catch (error) {
        setDownloadButtonIcon(button, 'cancel');
        const authorizationRequired = [
            'NEXUS_SSO_REQUIRED',
            'NEXUS_AUTH_REQUIRED'
        ].includes(error?.code)
            || (error?.code === 'NEXUS_AUTH_FAILED' && Number(error?.status) !== 403);
        const manual = !authorizationRequired && (
            error?.code === 'NEXUS_MANUAL_DOWNLOAD_REQUIRED'
            || Number(error?.status) === 403
            || /non-premium|website/i.test(error?.message || '')
        );
        updateModDownloadStatus({
            phase: manual ? 'manual' : 'failed',
            currentItem: manual ? item.title : error?.message || item.title
        });
        if (isNexusRateLimited(error)) {
            await htmlAlert(
                'Nexus Mods rate limit reached',
                formatRateLimitMessage(error),
                [{ text: 'OK', resolveWith: 'ok' }],
                'error'
            );
            return;
        }
        const choice = await htmlAlert(
            authorizationRequired
                ? 'Nexus Mods authorization required'
                : manual ? 'Download confirmation required' : 'Nexus Mods download failed',
            authorizationRequired
                ? 'Direct download needs Nexus Mods authorization. Connect with OAuth in Settings, then try again or continue on the official mod page.'
                : manual
                    ? 'Nexus Mods allows this download from its website, but not through the direct API for this account. Open the mod page to confirm the download, or import the archive after saving it.'
                : (error?.message || 'The archive could not be downloaded and imported.'),
            authorizationRequired || manual
                ? [
                    { text: 'Open Nexus Mods', resolveWith: 'open' },
                    ...(manual ? [{ text: 'Import archive', resolveWith: 'import' }] : []),
                    { text: 'Cancel', resolveWith: 'cancel' }
                ]
                : [{ text: 'OK', resolveWith: 'ok' }],
            authorizationRequired || manual ? undefined : 'error'
        );
        if (choice === 'open') {
            await window.communityAPI.modSources.open({ provider: 'nexus', url: item.sourceUrl });
        } else if (choice === 'import') {
            try {
                const imported = await window.deltamodBackend.invoke('importMod', []);
                if (imported) {
                    updateModDownloadStatus({ phase: 'complete', currentItem: item.title });
                }
            } catch (importError) {
                await htmlAlert(
                    'Import failed',
                    importError?.message || 'The downloaded archive could not be imported.',
                    [{ text: 'OK', resolveWith: 'ok' }],
                    'error'
                );
            }
        }
    } finally {
        unsubscribe();
        button.disabled = false;
        button.classList.remove('download-progress');
        button.style.removeProperty('--download-progress');
        if (button.textContent === '') button.innerHTML = original;
    }
}

const EXTERNAL_SOURCE_LABELS = Object.freeze({
    moddb: 'ModDB',
    nexus: 'Nexus Mods'
});

function externalSourceLabel(provider = SHOP_PROVIDER) {
    return EXTERNAL_SOURCE_LABELS[provider] || 'External source';
}

function renderExternalMods(table, result) {
    table.closest('table')?.classList.remove('is-state');
    table.replaceChildren();
    const items = Array.isArray(result?.items) ? result.items : [];
    const status = document.getElementById('contentFilterStatus');
    const attribution = document.getElementById('sourceAttribution');
    const providerLabel = externalSourceLabel();
    status.innerText = SHOP_PROVIDER === 'moddb'
        ? `Showing all ${items.length} download${items.length === 1 ? '' : 's'} exposed by ModDB's recent RSS feed.`
        : `Showing ${items.length} Nexus mod${items.length === 1 ? '' : 's'}.`;
    if (result?.stale) {
        status.innerText += ' Showing saved results because the live catalogue is unavailable.';
    } else if (result?.cached) {
        status.innerText += ' Loaded from the local catalogue cache.';
    }
    attribution.replaceChildren(document.createTextNode(result?.attribution || ''));
    if (SHOP_PROVIDER === 'moddb' && result?.catalogUrl) {
        const browseFullCatalog = document.createElement('a');
        browseFullCatalog.href = result.catalogUrl;
        browseFullCatalog.className = 'source-catalog-link';
        browseFullCatalog.innerText = `Browse the full ${providerLabel} catalogue`;
        browseFullCatalog.onclick = event => {
            event.preventDefault();
            return window.communityAPI.modSources.open({
                provider: SHOP_PROVIDER,
                url: result.catalogUrl
            });
        };
        attribution.append(' ', browseFullCatalog);
    }

    if (items.length === 0) {
        renderSourceState(
            table,
            'No mods found',
            SHOP_PROVIDER === 'moddb'
                ? 'The RSS feed contains only recent ModDB downloads. Older entries may still be available in the complete catalogue.'
                : 'Try another search or change the catalogue sort.',
            SHOP_PROVIDER === 'moddb' && result?.catalogUrl ? {
                label: `Browse full ${providerLabel} catalogue`,
                run: () => window.communityAPI.modSources.open({
                    provider: SHOP_PROVIDER,
                    url: result.catalogUrl
                })
            } : null
        );
        return;
    }

    for (const item of items) {
        const tr = document.createElement('tr');
        tr.dataset.canonicalIdentity = item.canonicalIdentity || `${item.provider}:${item.id}`;
        const info = document.createElement('td');
        info.style.display = 'flex';
        info.style.alignItems = 'center';
        info.style.gap = '14px';

        const image = document.createElement('img');
        image.className = 'modThumbImg';
        image.width = 130;
        image.height = 76;
        image.loading = 'lazy';
        image.alt = '';
        image.src = item.imageUrl || './img/mod-placeholder.png';
        image.onerror = () => {
            image.onerror = null;
            image.src = './img/mod-placeholder.png';
        };
        const previewImages = item.imageUrl ? [{
            urlA: item.imageUrl,
            urlB: item.imageUrl,
            urlCard220: item.imageUrl,
            urlCard530: item.imageUrl
        }] : [];
        const preview = createGalleryPreview(image, item.title, previewImages);

        const card = document.createElement('div');
        card.className = 'external-source-card';
        const title = document.createElement('span');
        title.className = 'modTitleSpan';
        title.innerText = item.title;
        const badge = document.createElement('span');
        badge.className = 'external-source-badge';
        badge.innerText = externalSourceLabel(item.provider);
        const featured = document.createElement('span');
        featured.className = 'external-featured-badge';
        featured.innerText = 'Featured';
        const meta = document.createElement('div');
        meta.className = 'modOtherInfoSpan calibri';
        meta.innerText = item.updatedAt
            ? `${item.author || `${badge.innerText} contributor`} · ${formatSourceDate(item.updatedAt)}`
            : (item.author || `${badge.innerText} contributor`);
        if (item.provider === 'nexus') {
            const popularity = document.createElement('span');
            popularity.className = 'external-source-popularity calibri';
            popularity.innerText = `${Number(item.downloads || 0).toLocaleString()} downloads · ${Number(item.endorsements || 0).toLocaleString()} endorsements`;
            meta.appendChild(popularity);
        }
        const summary = document.createElement('div');
        summary.className = 'external-source-summary calibri';
        summary.innerText = item.summary || 'No description was provided.';
        card.append(title, badge);
        if (item.featured) card.appendChild(featured);
        card.appendChild(meta);
        if (item.contentRating === 'adult') {
            const rating = document.createElement('span');
            rating.className = 'content-rating-chip';
            rating.innerText = 'Adult content';
            card.appendChild(rating);
        }
        card.appendChild(summary);
        info.append(preview, card);

        const actions = document.createElement('td');
        const actionGroup = document.createElement('div');
        actionGroup.className = 'external-source-actions';
        const primary = document.createElement('button');
        primary.type = 'button';
        const actionLabel = item.actionLabel || 'Open source page';
        primary.title = actionLabel;
        primary.setAttribute('aria-label', `${actionLabel}: ${item.title}`);
        primary.innerHTML = shopIcon(item.provider === 'nexus' ? 'download' : 'open');
        const canDownload = item.provider !== 'nexus'
            || window.deltamodBackend.isCommandAvailable('modSources:downloadNexus');
        primary.disabled = !canDownload;
        if (!canDownload) {
            primary.title = 'Direct Nexus downloads are unavailable in this app build';
            primary.setAttribute('aria-label', `Direct Nexus download unavailable: ${item.title}`);
        }
        primary.onclick = () => item.provider === 'nexus'
            ? downloadNexusSource(item, primary)
            : window.communityAPI.modSources.open({ provider: item.provider, url: item.sourceUrl });

        const open = document.createElement('button');
        open.type = 'button';
        open.title = 'Open source page';
        open.setAttribute('aria-label', `Open ${item.title} on ${badge.innerText}`);
        open.innerHTML = shopIcon('open');
        open.onclick = () => window.communityAPI.modSources.open({
            provider: item.provider,
            url: item.sourceUrl
        });
        actionGroup.append(primary);
        if (item.provider === 'nexus') actionGroup.append(open);
        actions.appendChild(actionGroup);
        tr.append(info, actions);
        table.appendChild(tr);
    }
}

async function initializeExternalSource(table) {
    setExternalSourceControlsDisabled(true);
    const query = String(window._pageArguments?.sourceQuery || '').trim();
    const sort = document.getElementById('nexusSort')?.value || 'latest_added';
    if (query) {
        document.getElementById('searchInput').value = query;
        syncSearchClearButton();
        const searchIndicator = document.getElementById('searchInd');
        searchIndicator.style.display = 'block';
        searchIndicator.innerText = `Currently showing results for "${query}"`;
    }
    renderSourceLoading(table);
    try {
        const request = {
            provider: SHOP_PROVIDER,
            gameId: SHOP_GAME_ID,
            query,
            sort
        };
        const response = await requestExternalSource(request);
        if (!response?.ok) {
            const error = new Error(
                response?.error?.message || 'The selected mod catalogue could not be loaded.'
            );
            error.code = response?.error?.code || 'MOD_SOURCE_BROWSE_FAILED';
            if (Number.isInteger(response?.error?.status)) {
                error.status = response.error.status;
            }
            copyRateLimitMetadata(error, response?.error);
            throw error;
        }
        if (isCurrentShopPage()) {
            renderExternalMods(table, response.result);
            setExternalSourceControlsDisabled(false);
        }
    } catch (error) {
        if (!isCurrentShopPage()) return;
        if (isNexusRateLimited(error)) {
            setExternalSourceControlsDisabled(true);
            renderSourceState(table, 'Nexus Mods rate limit reached', formatRateLimitMessage(error));
            scheduleNexusRateLimitRetry(table, error);
            return;
        }
        const needsNexusAuth = [
            'NEXUS_SSO_REQUIRED',
            'NEXUS_AUTH_REQUIRED',
            'NEXUS_AUTH_FAILED'
        ].includes(error?.code);
        setExternalSourceControlsDisabled(needsNexusAuth);
        renderSourceState(
            table,
            needsNexusAuth ? 'Sign in with Nexus Mods' : 'Catalogue unavailable',
            needsNexusAuth
                ? 'Connect your Nexus Mods account with OAuth in Settings before browsing this catalogue.'
                : (error?.message || 'The selected mod catalogue could not be loaded.'),
            needsNexusAuth ? {
                label: 'Open Nexus Mods settings',
                run: () => {
                    window._pageArguments = { cat: 'nexus' };
                    page('options');
                }
            } : {
                label: 'Retry',
                run: () => initializeExternalSource(table)
            }
        );
    }
}

async function plusPage(amt) {
    if (!gameBananaInitialLoadComplete || gameBananaRetryPending || !gameBananaHasMore) return;
    if (!Number.isFinite(amt) || amt <= 0) return;
    return loadGameBananaPage(window.PAGE + amt);
}

async function loadGameBananaPage(requestedPage) {
    if (
        !isCurrentShopPage() ||
        gameBananaPageRequestActive ||
        typeof window.currentPageStack?.GB_API !== 'string' ||
        !window.currentPageStack?.table?.isConnected
    ) {
        return;
    }
    if (!Number.isFinite(window.PAGE)) window.PAGE = 1;
    const previousPage = window.PAGE;
    window.PAGE = requestedPage;
    gameBananaPageRequestActive = true;
    gameBananaRetryPending = false;
    const { table, GB_API, filter, gameID } = window.currentPageStack;
    if (firstgeneration) renderSourceLoading(table);
    table.querySelector('.shop-page-retry')?.remove();
    table.setAttribute('aria-busy', 'true');
    try {
        const loaded = await renderMods(table, GB_API, filter, gameID);
        if (!isCurrentShopPage()) return;
        if (loaded) gameBananaInitialLoadComplete = true;
        else window.PAGE = previousPage;
    } finally {
        gameBananaPageRequestActive = false;
        table.removeAttribute('aria-busy');
    }
}

(async () => {
    let table = document.getElementById('modsBody');
    const sourceSelect = document.getElementById('modSourceSelect');
    const gameSelect = document.getElementById('modGameSelect');
    try {
        const { supportedGames } = await resolveShopGame();
        if (!isCurrentShopPage()) return;
        gameSelect.replaceChildren();
        for (const game of supportedGames) {
            const option = document.createElement('option');
            option.value = game.id;
            option.innerText = game.name;
            gameSelect.appendChild(option);
        }
        gameSelect.value = SHOP_GAME_ID;
        gameSelect.addEventListener('change', () => {
            const selectedGame = supportedGames.find(game => game.id === gameSelect.value);
            if (!selectedGame) return;
            localStorage.setItem('modShopGameId', selectedGame.id);
            const providers = availableProvidersForGame(selectedGame);
            const provider = providers.some(item => item.id === SHOP_PROVIDER)
                ? SHOP_PROVIDER
                : providers[0]?.id || 'gamebanana';
            localStorage.setItem('modShopProvider', provider);
            window._pageArguments = { gameId: selectedGame.id, provider };
            page('gamebanana-browse');
        });

        const availableProviders = availableProvidersForGame(SHOP_GAME);
        if (!availableProviders.some(provider => provider.id === SHOP_PROVIDER)) {
            SHOP_PROVIDER = availableProviders[0]?.id || 'gamebanana';
        }
        sourceSelect.replaceChildren();
        for (const provider of availableProviders) {
            const option = document.createElement('option');
            option.value = provider.id;
            option.innerText = provider.name;
            sourceSelect.appendChild(option);
        }
        sourceSelect.value = SHOP_PROVIDER;
        sourceSelect.addEventListener('change', () => {
            localStorage.setItem('modShopProvider', sourceSelect.value);
            window._pageArguments = { gameId: SHOP_GAME_ID, provider: sourceSelect.value };
            page('gamebanana-browse');
        });
        localStorage.setItem('modShopProvider', SHOP_PROVIDER);

    const isGameBanana = SHOP_PROVIDER === 'gamebanana';
    if (SHOP_PROVIDER === 'moddb') {
        document.getElementById('searchInput').placeholder = 'Search recent ModDB downloads…';
    }
    document.getElementById('gamebananaFilterControls').hidden = !isGameBanana;
    document.getElementById('nexusSortControls').hidden = SHOP_PROVIDER !== 'nexus';
    document.getElementById('gbPic').hidden = true;
    document.querySelector('.scrollBottomDetector').style.display = isGameBanana ? '' : 'none';

    if (!isGameBanana) {
        observer.disconnect();
        const nexusSort = document.getElementById('nexusSort');
        nexusSort.value = localStorage.getItem('nexusModSortV2') || 'trending';
        nexusSort.addEventListener('change', () => {
            localStorage.setItem('nexusModSortV2', nexusSort.value);
            initializeExternalSource(table);
        });
        window.currentPageStack = {
            ...window.currentPageStack,
            table,
            provider: SHOP_PROVIDER
        };
        await initializeExternalSource(table);
        genbtnstyles();
        return;
    }

    const gameID = currentShopGameBananaId();
    if (!gameID) throw new Error('The selected game is not mapped to GameBanana.');
    let GB_API = 'https://gamebanana.com/apiv11/Game/' + gameID + '/Subfeed?_sSort=default&_nPage=$PAGE';
    const contentRatingFilter = document.getElementById('contentRatingFilter');
    contentRatingFilter.value = currentContentFilter();
    contentRatingFilter.addEventListener('change', () => {
        localStorage.setItem('gamebananaContentFilter', contentRatingFilter.value);
        window._pageArguments = preserveShopArguments({
            lp: '1',
            gbAPI: capi || undefined,
            gbAPIFilter: window.currentPageStack.filter,
            leSearchQuery: csearch || undefined
        });
        page('gamebanana-browse');
    });
    let filter = async function(a) {
        return a;
    };

    if (window._pageArguments && window._pageArguments.gbAPI && window._pageArguments.gbAPIFilter) {
        GB_API = window._pageArguments.gbAPI;
        filter = window._pageArguments.gbAPIFilter;
    }

    if (window._pageArguments && window._pageArguments.leSearchQuery) {
        document.getElementById('searchInput').value = window._pageArguments.leSearchQuery;
        csearch = window._pageArguments.leSearchQuery;
        syncSearchClearButton();

        let searchInd = document.getElementById('searchInd');
        searchInd.style.display = 'block';
        searchInd.innerText = `Currently showing results for "${csearch}"`;
    }

    capi = GB_API;
    window._pageArguments = {}; // reset page arguments

    await gameBananaLogin();
    if (!isCurrentShopPage()) return;

    renderSourceLoading(table);

    window.currentPageStack = {
        ...window.currentPageStack,
        table,
        GB_API,
        filter,
        gameID
    };

    await loadGameBananaPage(window.PAGE);

    genbtnstyles();
    } catch (error) {
        console.error('Mod Shop initialization failed:', error);
        if (isCurrentShopPage()) {
            setExternalSourceControlsDisabled(false);
            sourceSelect.disabled = false;
            gameSelect.disabled = false;
            renderSourceState(
                table,
                'Mod Shop could not be loaded',
                error?.message || 'The Mod Shop failed to initialize.',
                { label: 'Retry', run: () => {
                    window._pageArguments = preserveShopArguments();
                    page('gamebanana-browse');
                } }
            );
        }
    }
})();

var searchel = document.getElementById('searchInput');
var autocomplete = document.querySelector('.autocomplete .results');
var clearSearchButton = document.getElementById('clearModSearchButton');
clearSearchButton.addEventListener('click', clearModSearch);
searchel.addEventListener('input', syncSearchClearButton);
searchel.addEventListener('keydown', function (event) {
    if (event.key === 'Escape' && searchel.value) {
        event.preventDefault();
        clearModSearch();
    }
});
syncSearchClearButton();
searchel.addEventListener('keypress', function (e) {
    if (e.key === 'Enter') {
        search();
    }
});

// Suggestions are input-driven, cancel stale requests, and use the active game.
let suggestionTimer;
let suggestionRequest;
let suggestionGame;
let suggestionSequence = 0;
let activeSuggestion = -1;
autocomplete.id = 'mod-search-suggestions';
autocomplete.setAttribute('role', 'listbox');
searchel.setAttribute('role', 'combobox');
searchel.setAttribute('aria-autocomplete', 'list');
searchel.setAttribute('aria-controls', autocomplete.id);
searchel.setAttribute('aria-expanded', 'false');
function closeSuggestions() {
    suggestionSequence++;
    clearTimeout(suggestionTimer);
    suggestionRequest?.abort();
    activeSuggestion = -1;
    autocomplete.replaceChildren();
    autocomplete.style.opacity = '0';
    autocomplete.style.pointerEvents = 'none';
    searchel.setAttribute('aria-expanded', 'false');
    searchel.removeAttribute('aria-activedescendant');
}
function scheduleSuggestions() {
    closeSuggestions();
    const query = searchel.value.trim();
    if (SHOP_PROVIDER !== 'gamebanana' || query.length < 3) return;
    const sequence = suggestionSequence;
    suggestionTimer = setTimeout(async () => {
        const request = new AbortController();
        suggestionRequest = request;
        try {
            suggestionGame ||= Promise.resolve(currentShopGameBananaId());
            const gameID = await suggestionGame;
            if (!gameID || sequence !== suggestionSequence || !isCurrentShopPage()) return;
            const response = await fetch('https://gamebanana.com/apiv12/Util/Search/Suggestions?_idGameRow='
                + encodeURIComponent(gameID) + '&_sSearchString=' + encodeURIComponent(query), { signal: request.signal });
            if (!response.ok) return;
            const results = await response.json();
            if (sequence !== suggestionSequence || !isCurrentShopPage() || document.activeElement !== searchel) return;
            if (!Array.isArray(results)) return;
            const fragment = document.createDocumentFragment();
            results.filter(item => typeof item === 'string').slice(0, 8).forEach((item, index) => {
                const result = document.createElement('div');
                result.className = 'result';
                result.id = `mod-suggestion-${index}`;
                result.setAttribute('role', 'option');
                result.setAttribute('aria-selected', 'false');
                result.textContent = item;
                // Keep focus in the combobox until its click is handled.
                result.addEventListener('mousedown', event => event.preventDefault());
                result.addEventListener('click', () => {
                    searchel.value = item;
                    closeSuggestions();
                    syncSearchClearButton();
                    search(item);
                });
                fragment.append(result);
            });
            autocomplete.replaceChildren(fragment);
            const open = autocomplete.childElementCount > 0;
            autocomplete.style.opacity = open ? '1' : '0';
            autocomplete.style.pointerEvents = open ? 'auto' : 'none';
            searchel.setAttribute('aria-expanded', String(open));
        } catch (_) {
            // Suggestions are optional. Full search remains available on failures.
        }
    }, 240);
}
searchel.addEventListener('input', scheduleSuggestions);
searchel.addEventListener('focus', scheduleSuggestions);
searchel.addEventListener('blur', closeSuggestions);
clearSearchButton.addEventListener('click', closeSuggestions);
searchel.addEventListener('keydown', event => {
    const options = [...autocomplete.children];
    if (event.key === 'Escape') { closeSuggestions(); return; }
    if (options.length && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
        event.preventDefault();
        activeSuggestion = (activeSuggestion + (event.key === 'ArrowDown' ? 1 : options.length - 1) + options.length) % options.length;
        options.forEach((option, index) => option.setAttribute('aria-selected', String(index === activeSuggestion)));
        searchel.setAttribute('aria-activedescendant', options[activeSuggestion].id);
    }
    if (event.key === 'Enter' && activeSuggestion >= 0 && options[activeSuggestion]) {
        event.preventDefault();
        options[activeSuggestion].click();
    }
});
window._onClosePage.push(closeSuggestions);
})();
