/**
 * ==========================================
 * Deltamod Core Script
 * ==========================================
 */


// Global Variables & State
var audio = new Audio();
audio.preload = "none";
var currentAudio = "";
var theme = null;
var pageN = null;
var addedStyle = null;
var currentPageScript = null;
var update = false;
var TARGET_MUSIC_VOLUME = 0.5;
var cmode = false; // Controller Mode

function releaseAudioBuffer() {
    audio.pause();
    audio.currentTime = 0;
    audio.removeAttribute('src');
    audio.load();
    currentAudio = "";
}

function loopMenuAudio() {
    const loopBuffer = 0.44;
    if (Number.isFinite(audio.duration) && audio.currentTime > audio.duration - loopBuffer) {
        audio.currentTime = 0;
        audio.play().catch(() => {});
    }
}

audio.addEventListener('timeupdate', loopMenuAudio);

window._onClosePage = window._onClosePage || [];

const PAGE_STYLESHEET_OVERRIDES = Object.freeze({
    allmods: './views/main/main.css'
});

const PAGE_REGISTRY = Object.freeze(Object.fromEntries([
    'allmods',
    'collection-exportchoose',
    'collections',
    'credits',
    'deleteall',
    'gamebanana-browse',
    'gamebanana-leave-comment',
    'goc-dl',
    'installmanager',
    'locate',
    'main',
    'options',
    'patching',
    'themesel'
].map(name => [name, Object.freeze({
    name,
    html: `./views/${name}/index.html`,
    script: `./views/${name}/index.js`,
    base: `./views/${name}/`,
    stylesheet: PAGE_STYLESHEET_OVERRIDES[name] || null
})])));

function loadPageScript(pageDefinition) {
    if (currentPageScript) {
        currentPageScript.remove();
        currentPageScript = null;
    }

    return new Promise((resolve, reject) => {
        const script = document.createElement('script');
        script.src = pageDefinition.script;
        script.async = false;
        script.dataset.pageScript = pageDefinition.name;
        script.addEventListener('load', () => resolve(), { once: true });
        script.addEventListener(
            'error',
            () => reject(new Error(`Failed to load script for page: ${pageDefinition.name}`)),
            { once: true }
        );
        currentPageScript = script;
        document.body.appendChild(script);
    });
}

/**
 * Wrapper for invoking Electron IPC calls.
 */
async function invoke(...params) {
    return window.electronAPI.invoke(...params);
}

/**
 * Generates and appends glyph icons to the DOM.
 * @param {Array} jsonArr - Array of glyph objects { icon, description }
 */
async function makeGlyphs(jsonArr) {
    var glyphContainer = document.querySelector('.glyph');
    glyphContainer.innerHTML = '';
    
    jsonArr.forEach(glyph => {
        var glyphIconElement = document.createElement('span');
        glyphIconElement.classList.add('material-symbols-outlined');
        glyphIconElement.innerText = glyph.icon;

        var glyphDescElement = document.createElement('span');
        glyphDescElement.innerText = glyph.description;

        glyphContainer.appendChild(glyphIconElement);
        glyphContainer.appendChild(glyphDescElement);
    });
}

async function reapplyHAStyles() {
    var alignment = localStorage.getItem('alertAlignment') || 'Bottom';
    if (!localStorage.getItem('alertAlignment')) {
        localStorage.setItem('alertAlignment', alignment);
    }
    var haDynamicStyle = document.getElementById('haDynamicStyle');
    if (!haDynamicStyle) {
        haDynamicStyle = document.createElement('style');
        haDynamicStyle.id = 'haDynamicStyle';
        document.head.appendChild(haDynamicStyle);
    }
    haDynamicStyle.innerHTML = await fetch('./haAlignments/' + alignment + '.css').then(res => res.text());
}

/**
 * Prompts the user to leave Controller Mode.
 */
async function promptLeaveCMode() {
    htmlAlert(
        "Exit controller mode", 
        "Are you sure you want to exit controller mode? If you exit controller mode, Deltamod will close.", 
        [
            { text: "Yes", resolveWith: true },
            { text: "No", resolveWith: false }
        ], 
        'stadia_controller'
    ).then((result) => {
        if (result) {
            window.electronAPI.invoke('cmode-off', []);
        }
    });
}

/**
 * Plays the "rew" SFX.
 */
async function rew() {
    if (await window.electronAPI.invoke('getUniqueFlag', ["SFX"]) === false) {
        return;
    }
    var a = new Audio();
    a.src = 'audio/rew.mp3';
    a.play();
}

/**
 * Brightens an RGB color by a specified amount.
 */
function brightenColor(r, g, b, amount) {
    r = Math.min(255, r + amount);
    g = Math.min(255, g + amount);
    b = Math.min(255, b + amount);
    return `rgb(${r}, ${g}, ${b})`;
}

// Window Management
function toggleFullscreen() { window.electronAPI.invoke('toggleFullscreen', []); }
function toggleMinimize() { window.electronAPI.invoke('minimizeMe', []); }
function genbtnstyles() { /* deprecated */ }

// Play rewind SFX on specific preload trigger
window.preloadAPI.onWRA(() => rew());

function error() {
    fetch('http://google.com'); // Force an error trigger if used in a specific context
}

function disableElem(elem) {
    if (!elem) return;
    elem.style.pointerEvents = 'none';
    elem.style.opacity = '0';
}
function enableElem(elem) {
    if (!elem) return;
    elem.style.pointerEvents = 'auto';
    elem.style.opacity = '1';
}

/**
 * ==========================================
 * Custom HTML Alert System
 * ==========================================
 */
var alertCache = [];
var isAlertShowing = false;

/**
 * Queues or displays an HTML-based alert dialog.
 */
async function htmlAlert(title, message, buttons, specialIcon) {
    if (isAlertShowing) {
        return new Promise((resolve, reject) => {
            alertCache.push({ title, message, buttons, resolve, reject, specialIcon: 'info' });
        });
    } else {
        return htmlAlertRaw(title, message, buttons, specialIcon);
    }
}

/**
 * Internal function to handle the rendering of the HTML alert.
 */
async function htmlAlertRaw(title, message, buttons, specialIcon = 'info') {
    return new Promise(async (resolve, reject) => {
        isAlertShowing = true;

        if (localStorage.getItem('alertAlignment') == "Separate") {
            var index = await window.electronAPI.invoke('htmlAlert_outwin', [title, message, buttons]);
            isAlertShowing = false;
            var button = buttons[index];

            if (button.resolveWith) {
                resolve(button.resolveWith);
                return;
            }

            if (button.rejectWith) {
                reject(button.rejectWith);
                return;
            }

            return;
        }

        var alertMain = document.getElementsByClassName('alertMain')[0];
        var alertMsgR = alertMain.getElementsByClassName('alertMsg')[0];

        var animOptions = 'cubic-bezier(0.16, 1, 0.3, 1) forwards';
        var animLength = 0.6;

        alertMsgR.innerHTML = '';

        // Container
        var alertMsg = document.createElement('div');
        alertMsgR.appendChild(alertMsg);

        // Title
        var titleElement = document.createElement('h1');
        titleElement.innerText = title;
        titleElement.style.opacity = '0';
        
        // Message
        var messageElement = document.createElement('p');
        messageElement.textContent = String(message);
        messageElement.style.whiteSpace = 'pre-line';
        messageElement.style.opacity = '0';
        
        alertMsg.appendChild(titleElement);
        alertMsg.appendChild(messageElement);

        // Buttons
        var buttonsHTML = document.createElement('div');
        buttonsHTML.style.textAlign = 'right';
        buttonsHTML.classList.add('alertButtons');
        buttonsHTML.style.opacity = '0';
        buttonsHTML.style.display = 'flex';
        buttonsHTML.style.gap = '8px';
        buttonsHTML.style.justifyContent = 'flex-end';
        

        buttons.forEach((button) => {
            var btn = document.createElement('button');
            btn.textContent = button.text;
            btn.style.flex = '1 1 0';
            btn.onclick = function() {
                // Outro animation
                alertMsgR.style.animation = `${animLength}s alertFadeOut ${animOptions}`;
                setTimeout(() => {
                    alertMain.style.animation = '';
                    alertMain.style.display = 'none';
                    alertMsgR.style.animation = `${animLength}s alertFadeIn ${animOptions}`;
                    alertMsgR.innerHTML = '';
                }, 300);
                
                isAlertShowing = false;
                
                // Play dismiss SFX
                var a = new Audio();
                a.src = 'audio/booow.mp3';
                if (window.electronAPI.invoke('getUniqueFlag', ["SFX"]) === true) {
                    a.play();
                }

                // Resolve/Reject
                if (button.resolveWith) {
                    resolve(button.resolveWith);
                    return;
                }
                if (button.rejectWith) {
                    reject(button.rejectWith);
                    return;
                }
                if (button.onClick) button.onClick();

                // Process next alert in cache
                if (alertCache.length > 0) {
                    setTimeout(() => {
                        var nextAlert = alertCache.shift();
                        htmlAlertRaw(nextAlert.title, nextAlert.message, nextAlert.buttons)
                            .then(nextAlert.resolve)
                            .catch(nextAlert.reject);
                    }, 600);
                }
            };
            buttonsHTML.appendChild(btn);
        });

        alertMain.style.display = 'flex';
        alertMsg.appendChild(buttonsHTML);

        // Special Background Icon
        var bigIcon = document.createElement('span');
        bigIcon.classList.add('material-symbols-outlined', 'alertBigIcon');
        bigIcon.innerText = specialIcon;
        bigIcon.style.fontSize = '490px';
        bigIcon.style.position = 'absolute';
        bigIcon.style.top = '-140px';
        bigIcon.style.right = '-50px';
        bigIcon.style.opacity = '0.1';
        bigIcon.style.userSelect = 'none';
        bigIcon.style.pointerEvents = 'none';
        alertMsgR.appendChild(bigIcon);

        // Cascade Intro Animations
        setTimeout(() => { titleElement.style.animation = `${animLength*1.2}s stuffFadeIn ${animOptions}`; }, 200);
        setTimeout(() => { messageElement.style.animation = `${animLength*1.2}s stuffFadeIn ${animOptions}`; }, 300);
        setTimeout(() => { buttonsHTML.style.animation = `${animLength*1.2}s stuffFadeIn ${animOptions}`; }, 400);

        // Play alert SFX
        var a = new Audio();
        a.src = 'audio/htmlalert.mp3';
        a.playbackRate = 0.9;
        if (await window.electronAPI.invoke('getUniqueFlag', ["SFX"]) === true) {
            a.play();
        }
    });
}

function credits(funny) {
    page('credits');
}

/**
 * ==========================================
 * Preload API Listeners (Updates, Logging)
 * ==========================================
 */
window.preloadAPI.onUpdateAvailable((info) => {
    console.log('Update available:', info.version);
    update = true;
    window.ustack = {};
    window.ustack.updateInfo = info;

    htmlAlert(
        'Update available', 
        `A new version of Deltamod Community (${info.version}) is available for download. Do you wish to update?`,
        [
            { text: 'Yes', resolveWith: "a" },
            { text: 'No', rejectWith: "a" }
        ], 
        'update'
    ).then(async () => {
        await window.electronAPI.invoke('start-update', []);
    }).catch(async () => {
        await window.electronAPI.invoke('ignore-update', []);
    });
});

window.preloadAPI.onDLMODProgress((info) => window.currentPageStack.dlmod && window.currentPageStack.dlmod(info));
window.preloadAPI.onDDS((info) => window.currentPageStack.du && window.currentPageStack.du(info.percentage));
window.preloadAPI.onProtocolDownloadProgress((info) => window.currentPageStack.onDLP && window.currentPageStack.onDLP(info.percentage));
window.preloadAPI.onProfileImportProgress((info) => window.currentPageStack.profileImportProgress && window.currentPageStack.profileImportProgress(info));
window.preloadAPI.onHashProgress((info) => window.currentPageStack.hashProgress && window.currentPageStack.hashProgress(info));
window.preloadAPI.onRefresh(() => page(pageN));
window.preloadAPI.onUpdateProgress((info) => window.currentPageStack.u && window.currentPageStack.u(info.perc));
window.preloadAPI.onFinishedPatch(() => window.currentPageStack.fp && window.currentPageStack.fp());
window.preloadAPI.onGPL((message) => window.currentPageStack.gpl && window.currentPageStack.gpl(message));
window.preloadAPI.onPage((title) => page(title));
window.preloadAPI.onAudio((stat) => stat ? openAudio() : closeAudio());
window.preloadAPI.onLeaveControllerMode(() => promptLeaveCMode());

function sanitizeHTML(str) {
    var temp = document.createElement('div');
    temp.textContent = str;
    return temp.innerHTML;
}

function formatBytes(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return 'an unknown amount of space';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024;
        unit += 1;
    }
    return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

async function offerOfficialProfileImport() {
    try {
        const summary = await window.communityAPI.profile.summary();
        if (!summary.exists || summary.previousImport || localStorage.getItem('communityMigrationPromptDismissed') === 'true') {
            return false;
        }

        const choice = await htmlAlert(
            'Import your Deltamod data',
            `Deltamod ${summary.version || ''} was found with ${summary.installations} installation(s), ${summary.mods} mod(s), and ${summary.themes} custom theme(s). A safe Community copy needs ${formatBytes(summary.totalBytes)}. Official Deltamod will not be changed.`,
            [
                { text: 'Review and import', resolveWith: 'import' },
                { text: 'Later', resolveWith: 'later' }
            ],
            'database'
        );

        if (choice === 'import') {
            window._pageArguments = { cat: 'data' };
            await page('options');
            return true;
        }

        localStorage.setItem('communityMigrationPromptDismissed', 'true');
    } catch (error) {
        console.warn('Could not inspect the official Deltamod profile:', error.message || error);
    }
    return false;
}

// Override console methods to tunnel logs through Electron IPC
console.log = function(...args) { window.electronAPI.invoke('log', [args.join(' '), 'LOG', pageN]); };
console.warn = function(...args) { window.electronAPI.invoke('log', [args.join(' '), 'WARN', pageN]); };
console.error = function(...args) { window.electronAPI.invoke('log', [args.join(' '), 'ERROR', pageN]); };
console.info = function(...args) { window.electronAPI.invoke('log', [args.join(' '), 'INFO', pageN]); };

function uppercaseFirst(string) {
    return string.charAt(0).toUpperCase() + string.slice(1);
}

function adaptForIcons(element) {
    element.style.display = 'flex';
    element.style.alignItems = 'left';
    element.style.gap = '5px';
    element.style.justifyContent = 'left';
    return element;
}

function icon(name, fontSize) {
    return `<span class="material-symbols-outlined" style="font-size: ${fontSize}">${name}</span>`;
}

/**
 * ==========================================
 * Theme & Audio Rendering
 * ==========================================
 */

/**
 * Refreshes the application theme and applies background/music.
 * @param {boolean} refreshAudio - Whether to also reload and play the main theme song.
 */
async function themeRefresh(refreshAudio = true) {
    theme = await fetch('themeprot://data/' + (await window.electronAPI.invoke('getTheme', [])) + '.theme.json').then(response => response.json());
    document.getElementsByClassName('bg')[0].style.backgroundImage = 'url(themeprot://img/' + theme.background + ')';
    
    if (refreshAudio) {
        const shouldPlayAudio = await window.electronAPI.invoke('getUniqueFlag', ["AUDIO"]);
        if (shouldPlayAudio) {
            audio.pause();
            audio.currentTime = 0;
            audio.loop = true;
            audio.volume = TARGET_MUSIC_VOLUME;
            audio.src = 'themeprot://mus/' + theme.mainSong;
            currentAudio = 'mainTheme.mp3';
            await audio.play().catch(() => {});
        } else {
            releaseAudioBuffer();
        }
        await page(pageN);
    }
}

window.preloadAPI.onThemeChange(themeRefresh);

let lockRandoms = false;

function elisten(element, event, handler) {
    element.addEventListener(event, handler);
    window._eventListeners = window._eventListeners || [];
    window._eventListeners.push({ element, event, handler });
}

/**
 * Navigates to a specific internal page and processes HTML/CSS injections.
 * @param {string} name - The identifier of the page to load.
 */
async function page(name) {
    rew();

    // Clear existing intervals/listeners to prevent memory leaks
    try {
        window._intervals.forEach(clearInterval);
    } catch(e) {
        console.log('No intervals to clear');
    }
    window._intervals = [];

    window._eventListeners = window._eventListeners || [];
    window._eventListeners.forEach(({ element, event, handler }) => {
        element.removeEventListener(event, handler);
    });
    window._eventListeners = [];

    window._onClosePage = window._onClosePage || [];
    window._onClosePage.forEach(func => func());
    window._onClosePage = [];

    if (currentPageScript) {
        currentPageScript.remove();
        currentPageScript = null;
    }

    if (addedStyle) {
        addedStyle.remove();
        addedStyle = null;
    }
    
    if (name == "") {
        name = pageN;
    }

    // Prevents escaping to home if game is baked
    if (await window.electronAPI.invoke('isBaked', []) && name == 'main' && PAGE_REGISTRY.bakedhome) {
        name = 'bakedhome';
    }
    const pageDefinition = PAGE_REGISTRY[name];
    if (!pageDefinition) {
        throw new Error(`Blocked unknown page: ${String(name)}`);
    }

    // Keep navigation immediate. Animating the full viewport promotes every
    // blurred panel to a large compositor surface and retains substantial GPU
    // memory after each page change.
    const viewport = document.querySelector('.viewport');
    viewport.style.animation = 'none';
    window.electronAPI.invoke('showWindow', []);

    // Load theme if not yet initialized
    if (!theme) {
        await themeRefresh(false); 
    }

    window.currentPageStack = {};

    // Process Page HTML
    var purifiedHTML = await fetch(pageDefinition.html).then(response => {
        if (!response.ok) throw new Error(`Failed to load page ${name}: HTTP ${response.status}`);
        return response.text();
    });
    
    var runScripts = false;
    var changeAudio = false;

    // Detect and queue JS execution
    if (purifiedHTML.includes('JSL')) {
        purifiedHTML = purifiedHTML.replace('JSL', '');
        runScripts = true;
    }

    // Load internal stylesheet tags
    if (purifiedHTML.includes('STYLESHEET[')) {
        var stylesheetSrc = purifiedHTML.match(/STYLESHEET\[(.*?)\]/);
        if (stylesheetSrc && stylesheetSrc[1]) {
            if (addedStyle) addedStyle.remove();
            addedStyle = document.createElement('link');
            addedStyle.rel = 'stylesheet';
            addedStyle.dataset.pageStylesheet = name;
            if (pageDefinition.stylesheet) {
                addedStyle.href = pageDefinition.stylesheet;
            } else {
                if (!/^[a-z0-9_-]+$/i.test(stylesheetSrc[1])) {
                    throw new Error(`Blocked unsafe stylesheet name on page ${name}.`);
                }
                addedStyle.href = `${pageDefinition.base}${stylesheetSrc[1]}.css`;
            }
            document.getElementById('head').appendChild(addedStyle);
        }
        purifiedHTML = purifiedHTML.replace(/STYLESHEET\[(.*?)\]/g, '');
    } else if (addedStyle) {
        addedStyle.remove();
        addedStyle = null;
    }

    // Handle NO-SIDEBAR tag
    if (purifiedHTML.includes('NO-SIDEBAR')) {
        purifiedHTML = purifiedHTML.replace('NO-SIDEBAR', '');
        ([...Array.from(document.getElementsByClassName('sidebar-button')), ...Array.from(document.getElementsByClassName('gamebanana-account'))]).forEach(button => button.setAttribute('data-disabled', 'true'));
    } else {
        ([...Array.from(document.getElementsByClassName('sidebar-button')), ...Array.from(document.getElementsByClassName('gamebanana-account'))]).forEach(button => button.setAttribute('data-disabled', 'false'));
    }

    // Extract Title Tag
    var title = purifiedHTML.match(/TITLE\[(.*?)\]/);
    purifiedHTML = purifiedHTML.replace(/TITLE\[(.*?)\]/g, '');
    const pageTitle = title?.[1] || 'Deltamod Community';
    const pageTitleElement = document.getElementById('pageTitle');
    if (pageTitleElement) {
        pageTitleElement.textContent = pageTitle;
    }

    // Extract Exclude Audio Tag
    var themeAudioExclude = purifiedHTML.match(/THEME-AUDIO-EXCLUDE\[(.*?)\]/);
    purifiedHTML = purifiedHTML.replace(/THEME-AUDIO-EXCLUDE\[(.*?)\]/g, '');

    // Process Audio Tag
    if (true) {
        var audioSrc = purifiedHTML.match(/AUDIO\[(.*?)\]/);
        console.log('Audio source found:' + audioSrc);
        
        if (!audioSrc || !audioSrc[1]) {
            audioSrc = ['AUDIO[mainTheme.mp3]', 'mainTheme.mp3'];
        }
        if (theme.id == themeAudioExclude?.[1]) {
            audioSrc = ['AUDIO[mainTheme.mp3]', 'mainTheme.mp3'];
        }
        if (await window.electronAPI.invoke('getUniqueFlag', ["DYNAMUSIC"]) == false) {
            audioSrc = ['AUDIO[mainTheme.mp3]', 'mainTheme.mp3'];
        }

        const shouldPlayAudio = await window.electronAPI.invoke('getUniqueFlag', ["AUDIO"]);

        if (!shouldPlayAudio) {
            releaseAudioBuffer();
        } else if (audioSrc && audioSrc[1] && (audioSrc[1] !== currentAudio || !audio.src)) {
            currentAudio = audioSrc[1];
            audio.pause();
            audio.currentTime = 0;
            
            if (audioSrc[1] == 'mainTheme.mp3') {
                audio.src = 'themeprot://mus/' + theme.mainSong;
            } else {
                audio.src = './' + audioSrc[1];
            }

            audio.volume = TARGET_MUSIC_VOLUME;
            changeAudio = true;
        }

        if (shouldPlayAudio) {
            await audio.play().catch(() => {});
        }
        purifiedHTML = purifiedHTML.replace(/AUDIO\[(.*?)\]/g, '');
    }

    // Inject Viewport HTML
    const pageViewport = document.getElementsByClassName('viewport')[0];
    pageViewport.querySelectorAll('img, video, source').forEach(media => {
        media.removeAttribute('src');
        media.removeAttribute('srcset');
    });
    pageViewport.innerHTML = purifiedHTML;

    // Set Active Sidebar Button
    ([...Array.from(document.getElementsByClassName('sidebar-button')), ...Array.from(document.getElementsByClassName('gamebanana-account'))]).forEach(button => {
        if (button.getAttribute('data-page') === name) {
            button.classList.add('active');
            button.setAttribute('aria-current', 'page');
        } else {
            button.classList.remove('active');
            button.removeAttribute('aria-current');
        }
    });

    // Handle Scrolling
    try {
        const vp = document.getElementsByClassName('viewport')[0];
        if (vp && typeof vp.scrollTo === 'function') {
            vp.scrollTo({ top: 0, left: 0, behavior: 'auto' });
        } else {
            window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
        }
    } catch (e) {
        window.scrollTo(0, 0);
    }

    pageN = name;

    // Generate Dynamic CSS Colors based on Theme
    var rgbNumbers = {
        r: theme.color.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/)[1],
        g: theme.color.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/)[2],
        b: theme.color.match(/rgb\((\d+),\s*(\d+),\s*(\d+)\)/)[3],
    };

    var generatedCSS = `
    /* Generated by Deltamod */
    :root {
        --theme-color: ${theme.color};
        --theme-color-rgbaless: rgba(${rgbNumbers.r}, ${rgbNumbers.g}, ${rgbNumbers.b}, 0.8);
        --theme-color-point2: rgba(${rgbNumbers.r}, ${rgbNumbers.g}, ${rgbNumbers.b}, 0.2);
        --theme-color-point3: rgba(${rgbNumbers.r}, ${rgbNumbers.g}, ${rgbNumbers.b}, 0.3);
    }
    input, select {
        box-shadow: inset 0 0 0 1px rgba(${rgbNumbers.r}, ${rgbNumbers.g}, ${rgbNumbers.b}, 0.42);
    }
    input, progress {
        accent-color: ${theme.color};
    }
    .sidebar {
        border-color: ${theme.color};
    }

    ${theme.specialCSS || ''}
    `;

    // Inject Dynamic Style
    var styleTag = document.getElementById('dynamic-theme-styles');
    styleTag.textContent = generatedCSS;

    // Execute Page JS
    if (runScripts) {
        try {
            await loadPageScript(pageDefinition);
        } catch (error) {
            console.error('Error occurred while loading script for page:', name, error);
        }
    }

}

/**
 * ==========================================
 * Global Window Listeners
 * ==========================================
 */
window.addEventListener('blur', () => {
    document.documentElement.classList.add('window-inactive');
    if (audio) {
        audio.volume = 0;
    }
});

window.addEventListener('focus', async () => {
    document.documentElement.classList.remove('window-inactive');
    let shouldPlayAudio = await window.electronAPI.invoke('getUniqueFlag', ["AUDIO"]);
    if (audio && shouldPlayAudio) {
        audio.volume = TARGET_MUSIC_VOLUME;
    }
});

if (!window.electronAPI) {
    window.alert('This application cannot run in this environment.');
    window.close();
    window.location.href = 'about:blank';
}

var renderedUser = false;
async function renderuser() {
    if (!(await window.electronAPI.invoke('validateGamebananaToken')) || !navigator.onLine) {
        renderedUser = true;
        return;
    }
    var gbuser = await window.electronAPI.invoke('getGamebananaUserinfo', []);

    var gbaccount = document.querySelector('.gamebanana-account');
    gbaccount.replaceChildren();
    const avatar = document.createElement('img');
    avatar.src = typeof gbuser?._sAvatarUrl === 'string'
        ? gbuser._sAvatarUrl
        : './img/mod-placeholder.png';
    avatar.alt = '';
    avatar.width = 25;
    avatar.height = 25;
    avatar.style.borderRadius = '15px';
    avatar.onerror = () => {
        avatar.onerror = null;
        avatar.src = './img/mod-placeholder.png';
    };
    const username = document.createElement('span');
    username.textContent = String(gbuser?._sName || 'GameBanana user');
    username.className = 'gamebanana-account-name';
    gbaccount.append(avatar, username);
    gbaccount.style.opacity = '1';
    const openGameBananaSettings = async () => {
        if (gbaccount.getAttribute('data-disabled') === 'true') {
            return;
        }
        window._pageArguments = {
            cat: 'gb'
        };
        page('options');
    };
    gbaccount.addEventListener('click', openGameBananaSettings);
    gbaccount.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            openGameBananaSettings();
        }
    });

    renderedUser = true;
}

/**
 * ==========================================
 * Initialization Boot Sequence
 * ==========================================
 */
(async function() {
    await reapplyHAStyles();

    cmode = await window.electronAPI.invoke('isCMode', []);

    var ribbon = document.querySelectorAll('.sidebar-button');
    ribbon.forEach(r => {
        r.addEventListener('click', async () => {
            if (r.getAttribute('data-disabled') === 'true') {
                return;
            }
            page(r.getAttribute('data-page'));
        });
    });
    
    // Initialize Theme prior to initial page loads
    await themeRefresh(false); 

    // Setup Controller Mode visual adjustments
    if (cmode) {
        document.body.style.zoom = '120%';
        document.querySelector('.glyph').style.display = 'flex';
        document.querySelector('.minimize-button').style.display = 'none';
        document.querySelector('.maximize-button').style.display = 'none';

        makeGlyphs([
            { icon: 'game_stick_left', description: "Move cursor" },
            { icon: 'game_stick_right', description: "Scroll" },
            { icon: 'cancel', description: "Click" },
            { icon: 'square_circle', description: "Right click" },
        ]);

        if (!localStorage.getItem('seenCModeAlert')) {
            localStorage.setItem('seenCModeAlert', 'true');
            htmlAlert(
                "Controller mode enabled", 
                "Controller mode is now enabled! Use Deltamod with your controller. Your left stick can help you move your mouse.", 
                [{ text: "Ok" }], 
                'stadia_controller'
            );
        }
    } else {
        document.querySelector('.glyph').style.display = 'none';
        if ((await window.electronAPI.invoke('getOS', [])).platform != 'linux') {
            window.addEventListener("gamepadconnected", async (event) => {
                if (await window.electronAPI.invoke('getUniqueFlag', ["CONTROLLER"]) === false) {
                    return;
                }
                if (!event.gamepad.id.toLowerCase().includes('dualshock') && !event.gamepad.id.toLowerCase().includes('dualsense')) {
                    return;
                }
                var res = await htmlAlert(
                    "Controller mode",
                    "It looks like you have a controller connected. Do you want to enable controller mode? Controller mode allows you to use Deltamod with your controller. Your left stick can help you move your mouse.",
                    [
                        { text: "Yes", resolveWith: true },
                        { text: "No", resolveWith: false }
                    ]
                );

                if (res) {
                    invoke('cmode-on', []);
                }
            });
        }
    }

    if (await offerOfficialProfileImport()) return;

    var loaded = await window.electronAPI.invoke('loadedDeltarune',[]);

    if (await window.electronAPI.invoke('fetchSharedVariable',["gb1click"]) === true) {
        page('goc-dl');
        return;
    }

    await new Promise(resolve => {
        var int = setInterval(async () => {
            if (renderedUser) {
                clearInterval(int);
                resolve();
            }
        }, 50);
    });

    // Main App Branching Route
    if (loaded.loaded) {
        var available = await window.electronAPI.invoke('fireUpdate', []);
        console.log('Update check complete. Update available:', available);

        var im = await window.electronAPI.invoke('shouldGoIM', []);
        if (im) {
            await page('installmanager');
        } else {
            await page('main');
        }

        window.electronAPI.invoke('executeArgumentCmd',[]);
    } else {
        await page('locate');
        document.querySelectorAll('.sidebar-button').forEach(button => button.setAttribute('data-disabled', 'true'));
        window.electronAPI.invoke('executeArgumentCmd',[]);
    }
})();

function closeAudio() {
    if (audio) {
        audio.pause();
    }
}

function openAudio() {
    if (audio && audio.src) {
        audio.play().catch(error => {
            // Silently fail if audio play is blocked
        });
    }
}

/**
 * ==========================================
 * Late Execution Modules (Shop Checker)
 * ==========================================
 */
(async () => {
    renderuser();

    if ((await window.electronAPI.invoke('validateGamebananaToken'))) {
        document.getElementById('collectionsRibbon').style.display = 'inline-flex';
    }
})();
