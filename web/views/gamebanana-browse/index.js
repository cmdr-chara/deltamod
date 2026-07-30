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

window.PAGE = PAGE;

window._onClosePage.push(() => {
    pageActive = false;
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
    if (entry.isIntersecting && isCurrentShopPage()) {
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
        ar.push({
            urlA: './img/mod-placeholder.png',
            urlB: './img/mod-placeholder.png',
            urlCard220: './img/mod-placeholder.png',
            urlCard530: './img/mod-placeholder.png'
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
        window.electronAPI.invoke('validateGamebananaToken', []),
        new Promise(resolve => setTimeout(() => resolve(false), 5000))
    ]);

    isGBLoggedIn = loggedin;

    const accountPicture = document.getElementById('gbPic');
    accountPicture.onclick = async () => {
        window._pageArguments = {
            cat: 'gb'
        };
        page('options');
    };

    if (loggedin) {
        var pic = await window.electronAPI.invoke('getGamebananaPic',[]);
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

async function search(searchQuery = null) {
    let query = searchQuery || document.getElementById('searchInput').value;
    if (searchQuery) {
        document.getElementById('searchInput').value = searchQuery;
    }
    if (query.trim().length < 2) {
        await htmlAlert("Search query too short","Please enter at least 2 characters to search.",[{text:"Ok",resolveWith:'ok'}], 'error');
        return;
    }

    if (SHOP_PROVIDER !== 'gamebanana') {
        window._pageArguments = {
            provider: SHOP_PROVIDER,
            sourceQuery: query.trim()
        };
        page('gamebanana-browse');
        return;
    }

    let gameID = (await window.electronAPI.invoke('getCurrentGameInfo',[])).gamebanana.id;

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
    let gameID = (await window.electronAPI.invoke('getCurrentGameInfo',[])).gamebanana.id;
    // Why doesn't GB have a standard endpoint format for subs SMH
    window._pageArguments.gbAPI = 'https://gamebanana.com/apiv11/Game/' + gameID + '/TopSubs';
    window._pageArguments.gbAPIFilter = async function(data) {
        return {_aRecords: data.map(x => {
            x.featuredDataset = true;
            return x;
        })};
    } 
    page('gamebanana-browse');
}

window.currentPageStack.featured = featured;
window.currentPageStack.qms = {}; //queryme stack

async function dlmod(dlurl, buttonElem=null, modid, modmodel) {
    lockUs = true;
    Array.from(document.querySelectorAll('.sidebar-button')).forEach(e => e.disabled = true);
    let queryme = Math.random().toString(36).substring(2, 15);

    buttonElem.innerHTML = icon('search_activity', '0.9em');

    window.currentPageStack.qms[queryme] = function(info) {
        if (info.error) {
            lockUs = false;
            Array.from(document.querySelectorAll('.sidebar-button')).forEach(e => e.disabled = false);
            buttonElem.innerHTML = icon('cancel', '0.9em')
            return;
        }

        const p = Math.max(0, Math.min(100, Number(info.progress) || 0));
        buttonElem.style.transition = 'none';
        buttonElem.classList.add('download-progress');
        buttonElem.style.setProperty('--download-progress', `${p}%`);

    };

    try {
        await window.electronAPI.invoke('dlmodURL',[dlurl, queryme, modid, modmodel]);
        buttonElem.innerHTML = icon('done_outline', '0.9em');
    } catch (error) {
        buttonElem.innerHTML = icon('cancel', '0.9em');
        await htmlAlert(
            'Download failed',
            error?.message || 'The mod could not be downloaded or imported.',
            [{ text: 'OK', resolveWith: 'ok' }]
        );
    } finally {
        delete window.currentPageStack.qms[queryme];
        lockUs = false;
        Array.from(document.querySelectorAll('.sidebar-button')).forEach(e => e.disabled = false);
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

async function renderMods(table, GB_API, filter, gameID) {
    if (!isCurrentShopPage() || typeof GB_API !== 'string' || !table?.isConnected) {
        return;
    }
    if (window.PAGE == null) {
        window.PAGE = 1;
    }
    var furl = GB_API.replace('$PAGE', window.PAGE);
    console.log('Fetching from URL: ' + furl);
    var response = await fetch(furl);
    if (!isCurrentShopPage()) return;
    var data = await filter(await response.json());
    if (!isCurrentShopPage()) return;

    var featured = await fetch("https://gamebanana.com/apiv11/Game/" + gameID + "/TopSubs");
    if (!isCurrentShopPage()) return;
    var featuredData = await featured.json();
    if (!isCurrentShopPage()) return;
    var featuredIDs = featuredData.map(x => {return {id: x._idRow, period: x._sPeriod};});

    try {
        if (data._aMetadata._bIsComplete) {
            observer.disconnect(); // stop observing since there's no more content to load
            document.querySelector('.scrollBottomDetector').style.display = 'none'; // hide the loading indicator
        }

        const records = applyContentFilter(Array.isArray(data._aRecords) ? data._aRecords : []);

        if (records.length == 0 && firstgeneration) {
            var tr = document.createElement('tr');
            var td = document.createElement('td');
            td.colSpan = 2;
            td.innerText = currentContentFilter() === 'all'
                ? "No mods were found matching your query."
                : "No submissions on this page match the active Content filter.";
            tr.appendChild(td);
            table.appendChild(tr);
            observer.disconnect(); // stop observing since there's no more content to load
            document.querySelector('.scrollBottomDetector').style.display = 'none'; // hide the loading indicator
            return;
        }
        for (const mod of records) {
            await (async () => {
                var tr = document.createElement('tr');

                var td0 = document.createElement('td');
                td0.style.display = 'flex';
                td0.style.alignItems = 'flex-start';
                td0.style.gap = '8px';
                td0.style.justifyContent = 'left';
                // Rendering of td0
                {
                var div0 = document.createElement('div');
                div0.style.display = 'flex';
                div0.style.alignItems = 'center';
                div0.style.gap = '8px';
                div0.className = 'modThumbDiv';
                
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
                let i = 0;
                img.style.width = '130px';
                img.style.margin = '4px';
                img.style.aspectRatio = '16 / 9';
                img.style.borderRadius = '4px';
                img.style.border = '2px solid var(--theme-color)';
                img.style.height = 'auto';
                img.style.cursor = 'zoom-in';
                img.onclick = () => openImageLightbox(mod._sName, thumbs, i);
                img.style.objectFit = 'cover';
                img.style.transition = 'opacity 0.3s ease-in-out';
                img.style.objectPosition = 'center';

                var gridSmallImages = document.createElement('div');
                gridSmallImages.className = 'modThumbGrid';
                gridSmallImages.style.display = 'grid';
                gridSmallImages.style.gridTemplateColumns = 'repeat(3, 1fr)';
                gridSmallImages.style.gridTemplateRows = 'repeat(3, auto)';
                gridSmallImages.style.gap = '4px';
                gridSmallImages.style.marginTop = '4px';
                gridSmallImages.style.width = '100%';

                thumbs.slice(0, 9).forEach((thumb, index) => {
                    var smallImg = document.createElement('img');
                    smallImg.src = thumb.urlB;
                    smallImg.loading = 'lazy';
                    smallImg.decoding = 'async';
                    smallImg.alt = `${mod._sName || 'Mod'} thumbnail ${index + 1}`;
                    smallImg.onerror = () => {
                        smallImg.onerror = null;
                        smallImg.src = './img/mod-placeholder.png';
                    };
                    smallImg.style.width = '30px';
                    smallImg.style.aspectRatio = '16 / 9';
                    smallImg.style.objectFit = 'cover';
                    smallImg.style.objectPosition = 'center';
                    smallImg.style.borderRadius = '4px';
                    smallImg.style.border = '1px solid var(--theme-color)';
                    smallImg.onclick = async () => {
                        i = index;
                        img.style.opacity = '0';
                        await timeoutPromise(300);
                        setCardImageSource(img, thumb);
                        img.onload = () => {
                            img.style.opacity = '1';
                            img.onload = null; // Remove the onload handler after it has been called
                        }
                    }
                    smallImg.style.cursor = 'pointer';
                    gridSmallImages.appendChild(smallImg);
                });

                div0.appendChild(gridSmallImages);
                div0.appendChild(img);

                var div1 = document.createElement('div');
                div1.className = 'modCopy';
                div1.style.marginLeft = '8px';
                div1.style.display = 'flex';
                div1.style.flexDirection = 'column';
                div1.style.justifyContent = 'space-between';
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

                var otherInfoSpan = document.createElement('div');
                otherInfoSpan.className = 'modOtherInfoSpan';
                otherInfoSpan.style.fontSize = '0.9em';
                otherInfoSpan.style.display = 'flex';
                otherInfoSpan.style.flexDirection = 'column';
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
                        ["today","Best of today"],
                        ["week","Best of this week"],
                        ["month","Best of this month"],
                        ["3month","Best of last 3 months"],
                        ["6month","Best of last 6 months"],
                        ["year","Best of this year"],
                        ["alltime","All-time featured"]
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

                var date = new Date(Math.max(addDate, modDate) * 1000);

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

                var td1 = document.createElement('td');
                td1.style.textAlign = 'center';
                // Rendering of td1
                {
                    var dlBtn = document.createElement('button');
                    dlBtn.innerHTML = icon('download', '0.9em') + '';
                    dlBtn.className = 'serietast';
                    dlBtn.title = 'Download and import mod';
                    dlBtn.setAttribute('aria-label', `Download ${mod._sName}`);
                    dlBtn.onclick = async () => {
                        dlBtn.disabled = true;
                        dlBtn.innerHTML = icon('downloading', '0.9em');
                        dlBtn.style.opacity = '0.7';

                        var dlpage = await fetch(`https://gamebanana.com/apiv11/${mod._sModelName}/${mod._idRow}/ProfilePage`);
                        dlpage = await dlpage.json();

                        var eligibleDownloads = [];

                        dlpage._aFiles.forEach(file => {
                            try {
                                var mmo = file._aModManagerIntegrations.map(x => x._idToolRow);
                                if (mmo.includes(20575)) {
                                    eligibleDownloads.push(file);
                                }
                            }
                            catch {
                                //nothing, file is just not compatible
                            }
                        });

                        if (eligibleDownloads.length === 0) {
                            dlBtn.innerHTML = icon('cancel', '0.9em');
                            var open = await htmlAlert("One-click download not available", "This mod cannot be downloaded via Deltamod because the owner did not package it for usage with the tool.",[{text:"Ok",resolveWith:'no',},{text:"Open mod page on GameBanana",resolveWith:'yes'}], 'web_traffic');
                            if (open === 'yes') {
                                window.open(mod._sProfileUrl, '_blank');
                            }
                            return;
                        }

                        if (eligibleDownloads.length > 1) {
                            dlBtn.innerHTML = icon('indeterminate_question_box', '0.9em');
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
                                    dlmod(file._sDownloadUrl.replace('dl','mmdl'), dlBtn, mod._idRow, mod._sModelName);
                                    dtr.remove();
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

                            tr.insertAdjacentElement("afterend", dtr);

                            await timeoutPromise(100);
                            rew();

                            return;
                        }

                        dlmod(eligibleDownloads[0]._sDownloadUrl.replace('dl','mmdl'), dlBtn, mod._idRow, mod._sModelName);
                    };

                    td1.appendChild(dlBtn);

                    var commentBtn = document.createElement('button');
                    commentBtn.innerHTML = icon('comment', '0.9em') + '';
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
                    likeBtn.innerHTML = icon('mood_heart', '0.9em') + '';
                    likeBtn.style.marginLeft = '8px';
                    likeBtn.className = 'serietast';
                    likeBtn.title = isGBLoggedIn ? 'Like mod' : 'Log in to GameBanana to like mods';
                    likeBtn.setAttribute('aria-label', `Like ${mod._sName}`);
                    likeBtn.disabled = !isGBLoggedIn;
                    likeBtn.onclick = async () => {
                        let res = await window.electronAPI.invoke('gbLikeMod',[mod._sModelName, mod._idRow]);
                        if (res.status == 200) {
                            likeBtn.innerHTML = icon('sentiment_very_satisfied', '0.9em') + '';
                            likeBtn.disabled = true;
                        }
                        else if (res.data._sErrorCode.toLowerCase() == 'already_liked') {
                            await htmlAlert("Can't like the mod","You've already liked this mod. Can't get any more likes than that!",[{text:'Ok',resolveWith:'ok'}], 'sentiment_very_satisfied');
                            likeBtn.innerHTML = icon('sentiment_very_satisfied', '0.9em') + '';
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

                table.appendChild(tr);
            })();
        };
    }
    catch (e) {
        console.error(e);
        
        await htmlAlert("Error","An error occurred while loading mods from GameBanana. Please try again later.",[{text:'Ok',resolveWith:'ok'}], 'error');

        page('main');
        firstgeneration = true;
        return;
    }

    firstgeneration = false;
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

function formatSourceDate(value) {
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return 'Date unavailable';
    return new Intl.DateTimeFormat(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short'
    }).format(date);
}

function setExternalSourceControlsDisabled(disabled) {
    document.getElementById('searchInput').disabled = disabled;
    document.getElementById('modShopSearchButton').disabled = disabled;
    const sort = document.getElementById('nexusSort');
    if (sort) sort.disabled = disabled;
}

async function downloadNexusSource(item, button) {
    const operationId = crypto.randomUUID();
    const original = button.innerHTML;
    button.disabled = true;
    button.innerHTML = icon('progress_activity', '0.9em');
    const unsubscribe = window.communityAPI.modSources.onProgress(progress => {
        if (progress.operationId !== operationId) return;
        if (progress.phase === 'download' && progress.total > 0) {
            const percentage = Math.max(0, Math.min(100, (progress.completed / progress.total) * 100));
            button.classList.add('download-progress');
            button.style.setProperty('--download-progress', `${percentage}%`);
        }
    });
    try {
        await window.communityAPI.modSources.downloadNexus({
            modId: item.id,
            operationId,
            sourceUrl: item.sourceUrl
        });
        button.innerHTML = icon('done_outline', '0.9em');
    } catch (error) {
        button.innerHTML = icon('cancel', '0.9em');
        const manual = error?.code === 'NEXUS_MANUAL_DOWNLOAD_REQUIRED'
            || /non-premium|website/i.test(error?.message || '');
        const choice = await htmlAlert(
            manual ? 'Download confirmation required' : 'Nexus Mods download failed',
            manual
                ? 'Nexus Mods requires this download to be confirmed on its website. The mod page can be opened now.'
                : (error?.message || 'The archive could not be downloaded and imported.'),
            manual
                ? [
                    { text: 'Open mod page', resolveWith: 'open' },
                    { text: 'Cancel', resolveWith: 'cancel' }
                ]
                : [{ text: 'OK', resolveWith: 'ok' }],
            manual ? undefined : 'error'
        );
        if (choice === 'open') {
            await window.communityAPI.modSources.open({ provider: 'nexus', url: item.sourceUrl });
        }
    } finally {
        unsubscribe();
        button.disabled = false;
        button.classList.remove('download-progress');
        button.style.removeProperty('--download-progress');
        if (button.textContent === '') button.innerHTML = original;
    }
}

function renderExternalMods(table, result) {
    table.closest('table')?.classList.remove('is-state');
    table.replaceChildren();
    const items = Array.isArray(result?.items) ? result.items : [];
    const status = document.getElementById('contentFilterStatus');
    const attribution = document.getElementById('sourceAttribution');
    status.innerText = SHOP_PROVIDER === 'moddb'
        ? `Showing ${items.length} recent ModDB download${items.length === 1 ? '' : 's'} from the RSS feed.`
        : `Showing ${items.length} Nexus mod${items.length === 1 ? '' : 's'}.`;
    attribution.replaceChildren(document.createTextNode(result?.attribution || ''));
    if (SHOP_PROVIDER === 'moddb' && result?.catalogUrl) {
        const browseFullCatalog = document.createElement('a');
        browseFullCatalog.href = result.catalogUrl;
        browseFullCatalog.className = 'source-catalog-link';
        browseFullCatalog.innerText = 'Browse the full ModDB catalogue';
        browseFullCatalog.onclick = event => {
            event.preventDefault();
            return window.communityAPI.modSources.open({
            provider: 'moddb',
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
                label: 'Browse full ModDB catalogue',
                run: () => window.communityAPI.modSources.open({
                    provider: 'moddb',
                    url: result.catalogUrl
                })
            } : null
        );
        return;
    }

    for (const item of items) {
        const tr = document.createElement('tr');
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
        if (item.imageUrl) {
            image.tabIndex = 0;
            image.setAttribute('role', 'button');
            image.setAttribute('aria-label', `Preview ${item.title}`);
            image.onclick = () => openImageLightbox(item.title, [{
                urlA: item.imageUrl,
                urlB: item.imageUrl,
                urlCard220: item.imageUrl,
                urlCard530: item.imageUrl
            }]);
            image.onkeydown = event => {
                if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    image.click();
                }
            };
        }

        const card = document.createElement('div');
        card.className = 'external-source-card';
        const title = document.createElement('span');
        title.className = 'modTitleSpan';
        title.innerText = item.title;
        const badge = document.createElement('span');
        badge.className = 'external-source-badge';
        badge.innerText = SHOP_PROVIDER === 'moddb' ? 'ModDB' : 'Nexus Mods';
        const meta = document.createElement('div');
        meta.className = 'modOtherInfoSpan calibri';
        meta.innerText = `${item.author} · ${formatSourceDate(item.updatedAt)}`;
        const summary = document.createElement('div');
        summary.className = 'external-source-summary calibri';
        summary.innerText = item.summary || 'No description was provided.';
        card.append(title, badge, meta);
        if (item.contentRating === 'adult') {
            const rating = document.createElement('span');
            rating.className = 'content-rating-chip';
            rating.innerText = 'Adult content';
            card.appendChild(rating);
        }
        card.appendChild(summary);
        info.append(image, card);

        const actions = document.createElement('td');
        const actionGroup = document.createElement('div');
        actionGroup.className = 'external-source-actions';
        const primary = document.createElement('button');
        primary.type = 'button';
        primary.title = item.actionLabel;
        primary.setAttribute('aria-label', `${item.actionLabel}: ${item.title}`);
        primary.innerHTML = icon(item.provider === 'nexus' ? 'download' : 'open_in_new', '0.9em');
        primary.onclick = () => item.provider === 'nexus'
            ? downloadNexusSource(item, primary)
            : window.communityAPI.modSources.open({ provider: item.provider, url: item.sourceUrl });

        const open = document.createElement('button');
        open.type = 'button';
        open.title = 'Open source page';
        open.setAttribute('aria-label', `Open ${item.title} on ${badge.innerText}`);
        open.innerHTML = icon('open_in_new', '0.9em');
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
    setExternalSourceControlsDisabled(false);
    const query = String(window._pageArguments?.sourceQuery || '').trim();
    const sort = document.getElementById('nexusSort')?.value || 'latest_added';
    if (query) {
        document.getElementById('searchInput').value = query;
        const searchIndicator = document.getElementById('searchInd');
        searchIndicator.style.display = 'block';
        searchIndicator.innerText = `Currently showing results for "${query}"`;
    }
    renderSourceState(table, 'Loading catalogue…', 'Fetching metadata from the selected provider.');
    try {
        const response = await window.communityAPI.modSources.browse({
            provider: SHOP_PROVIDER,
            query,
            sort
        });
        if (!response?.ok) {
            const error = new Error(
                response?.error?.message || 'The selected mod catalogue could not be loaded.'
            );
            error.code = response?.error?.code || 'MOD_SOURCE_BROWSE_FAILED';
            if (Number.isInteger(response?.error?.status)) {
                error.status = response.error.status;
            }
            throw error;
        }
        if (isCurrentShopPage()) renderExternalMods(table, response.result);
    } catch (error) {
        if (!isCurrentShopPage()) return;
        const needsNexusKey = error?.code === 'NEXUS_API_KEY_REQUIRED'
            || error?.code === 'NEXUS_AUTH_FAILED';
        setExternalSourceControlsDisabled(needsNexusKey);
        renderSourceState(
            table,
            needsNexusKey ? 'Connect Nexus Mods' : 'Catalogue unavailable',
            needsNexusKey
                ? 'Connect your Nexus Mods account in Settings before browsing this catalogue.'
                : (error?.message || 'The selected mod catalogue could not be loaded.'),
            needsNexusKey ? {
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
    if (
        !isCurrentShopPage() ||
        typeof window.currentPageStack?.GB_API !== 'string' ||
        !window.currentPageStack?.table?.isConnected
    ) {
        return;
    }
    if (!Number.isFinite(window.PAGE)) window.PAGE = 1;
    window.PAGE += amt;
    await renderMods(window.currentPageStack.table, window.currentPageStack.GB_API, window.currentPageStack.filter, window.currentPageStack.gameID);
}

(async () => {
    if (navigator.onLine === false) {
        await htmlAlert("You're offline","To access this page, you must have an active Internet connection.",[{text:"Ok",resolveWith:'ok'}], 'cloud_alert');
        page('main');
        return;
    }
    let table = document.getElementById('modsBody');
    const sourceSelect = document.getElementById('modSourceSelect');
    const providers = await window.communityAPI.modSources.providers();
    const availableProviders = providers.filter(provider => provider.available);
    if (!availableProviders.some(provider => provider.id === SHOP_PROVIDER)) {
        SHOP_PROVIDER = availableProviders[0]?.id || 'gamebanana';
    }
    for (const provider of availableProviders) {
        const option = document.createElement('option');
        option.value = provider.id;
        option.innerText = provider.name;
        sourceSelect.appendChild(option);
    }
    sourceSelect.value = SHOP_PROVIDER;
    sourceSelect.addEventListener('change', () => {
        localStorage.setItem('modShopProvider', sourceSelect.value);
        window._pageArguments = { provider: sourceSelect.value };
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
        nexusSort.value = localStorage.getItem('nexusModSort') || 'latest_added';
        nexusSort.addEventListener('change', () => {
            localStorage.setItem('nexusModSort', nexusSort.value);
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

    let gameID = (await window.electronAPI.invoke('getCurrentGameInfo',[])).gamebanana.id;
    let GB_API = 'https://gamebanana.com/apiv11/Game/' + gameID + '/Subfeed?_sSort=default&_nPage=$PAGE';
    const contentRatingFilter = document.getElementById('contentRatingFilter');
    contentRatingFilter.value = currentContentFilter();
    contentRatingFilter.addEventListener('change', () => {
        localStorage.setItem('gamebananaContentFilter', contentRatingFilter.value);
        window._pageArguments = {
            lp: '1',
            gbAPI: capi || undefined,
            gbAPIFilter: window.currentPageStack.filter,
            leSearchQuery: csearch || undefined
        };
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

        let searchInd = document.getElementById('searchInd');
        searchInd.style.display = 'block';
        searchInd.innerText = `Currently showing results for "${csearch}"`;
    }

    capi = GB_API;
    window._pageArguments = {}; // reset page arguments

    await gameBananaLogin();

    table.innerHTML = '';

    window.currentPageStack = {
        ...window.currentPageStack,
        table,
        GB_API,
        filter,
        gameID
    };

    await renderMods(table, GB_API, filter, gameID);

    genbtnstyles();
})();

var searchel = document.getElementById('searchInput');
var autocomplete = document.querySelector('.autocomplete .results');
searchel.addEventListener('keypress', function (e) {
    if (e.key === 'Enter') {
        search();
    }
});

searchel.addEventListener('focus', function (e) {
    if (SHOP_PROVIDER !== 'gamebanana') return;
    autocomplete.style.opacity = '1';
    autocomplete.style.pointerEvents = 'auto';
});

searchel.addEventListener('blur', function (e) {
    setTimeout(() => {
        autocomplete.style.opacity = '0';
        autocomplete.style.pointerEvents = 'none';
    }, 300);
});

let sval = 0;

window._intervals = window._intervals || [];
window._intervals.push(setInterval(async () => {
    if (SHOP_PROVIDER !== 'gamebanana') return;
    var isFocused = document.activeElement === searchel;
    if (!isFocused) {
        autocomplete.style.opacity = '0';
        autocomplete.style.pointerEvents = 'none';
        return;
    }
    if (sval != searchel.value) {
        sval = searchel.value;
    } else return;

    if (searchel.value.length < 3) {
        autocomplete.innerHTML = '';
        autocomplete.style.opacity = '0';
        autocomplete.style.pointerEvents = 'none';
        return;
    }

    var res = await fetch('https://gamebanana.com/apiv12/Util/Search/Suggestions?_idGameRow=6755&_sSearchString=' + (searchel.value));
    var elems = JSON.parse(await res.text());

    autocomplete.style.opacity = '1';
    autocomplete.style.pointerEvents = 'auto';
    autocomplete.innerHTML = '';
    elems.forEach((item) => {
        var resultDiv = document.createElement('div');
        resultDiv.className = 'result';
        resultDiv.innerText = item;
        resultDiv.addEventListener('click', function () {
            searchel.value = item;
            search(searchel.value);
        });
        autocomplete.appendChild(resultDiv);
    });
    if (elems.length === 0) {
        var noResultDiv = document.createElement('div');
        noResultDiv.className = 'result';
        noResultDiv.innerText = 'No results found';
        noResultDiv.style.color = '#888';
        autocomplete.appendChild(noResultDiv);
        noResultDiv.style.pointerEvents = 'none';
    }
}, 1000));
})();
