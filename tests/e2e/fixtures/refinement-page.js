// Browser fixture: real original shell, styles and view scripts; native IPC is mocked.
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '../../..');
const web = path.join(root, 'web');
const read = file => fs.readFileSync(path.join(web, file), 'utf8');
// Inline only repository-owned static files, allowing offline sandbox execution.
const mime = require('mime-types');
function asset(file) {
    return fs.existsSync(file) ? `data:${mime.lookup(file)||'application/octet-stream'};base64,${fs.readFileSync(file).toString('base64')}` : '';
}
function cssText(file) {
    return fs.readFileSync(file,'utf8').replace(/url\(["']?([^\)"']+)["']?\)/g, (full,url) => {
        if (/^(data:|https?:|#)/.test(url)) return full;
        const candidate = path.resolve(path.dirname(file),url);
        return candidate.startsWith(web + path.sep) ? `url("${asset(candidate)}")` : full;
    });
}
async function openView(page, view, { count = 3, width = 1100 } = {}) {
    await page.setViewportSize({ width, height: 800 });
    const html = read('index.html').replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
        .replace(/<div\s+id="deltamod-boot-root"[\s\S]*?<\/div>/, '')
        .replace(/<link[^>]+href="([^"]+)"[^>]*>/g, (_,url) => {
            const file = path.join(web,url);
            return fs.existsSync(file) ? `<style>${cssText(file)}</style>` : '';
        }).replace(/src="(?!https?:|data:)([^"]+)"/g, (_,url) => `src="${asset(path.join(web,url))}"`);
    await page.setContent(html);
    const languages = Object.fromEntries(fs.readdirSync(path.join(web,'langs')).flatMap(code => {
        return ['language.json','metadata.txt'].flatMap(name => {
            const file = path.join(web,'langs',code,name);
            return fs.existsSync(file) ? [[`./langs/${code}/${name}`,fs.readFileSync(file,'utf8')]] : [];
        });
    }));
    const placeholder = asset(path.join(web,'img/mod-placeholder.png'));
    const background = asset(path.join(web,'themes/img/scarlet.png'));
    await page.evaluate(({languages,placeholder,background}) => {
        const stored = new Map();
        Object.defineProperty(window,'localStorage',{value:{getItem:key=>stored.get(key)||null,setItem:(key,value)=>stored.set(key,String(value)),removeItem:key=>stored.delete(key)}});
        window.fetch = async url => new Response(languages[String(url)] || '',{status:languages[String(url)] ? 200 : 404});
        window.__placeholder = placeholder;
        document.querySelector('.bg').style.backgroundImage = `url("${background}")`;
    },{languages,placeholder,background});
    await page.evaluate(({ count, view }) => {
        window._onClosePage = [];
        window._intervals = [];
        window._pageArguments = {};
        window.currentPageStack = {};
        window.pageN = view;
        window.__calls = [];
        window.__failSave = false;
        window.__states = {};
        window.__mods = Array.from({ length: count }, (_, i) => ({
            uid: `mod-${i}`, folder: `mod-${i}`, name: `Forest ${i+1}`, description:'An illustrated forest adventure.',
            author:['Chara', 'Forest team'], packageID:`community.forest${i}`, version:'1.2', game: i % 2 ? 'other' : 'undertale',
            size:i+1, mergeSupport:true, gamebanana:{supports:false}, isIncompatible:false
        }));
        window.genbtnstyles = () => {};
        window.rew = () => {};
        window.tippy = () => {};
        window.sanitizeHTML = value => value;
        window.adaptForIcons = node => node;
        window.icon = name => `<span class="material-symbols-outlined" aria-hidden="true">${name}</span>`;
        window.elisten = (node, event, handler) => {
            node.addEventListener(event, handler);
            window._onClosePage.push(() => node.removeEventListener(event, handler));
        };
        window.page = async target => { window.__pageTarget = target; };
        window.deltamodBackend = {
            assetUrl: () => window.__placeholder,
            isCommandAvailable: () => false,
            invoke: async (channel, args = []) => {
                window.__calls.push([channel, args]);
                if (channel === 'getModList') return {modList:window.__mods,errors:[]};
                if (channel === 'getModState') return window.__states[args[0]] || false;
                if (channel === 'toggleModState') { window.__states[args[0]] = args[1]; return true; }
                if (channel === 'getModImage') return {path:window.__placeholder};
                if (channel === 'getAvailableGames') return [{id:'undertale',name:'UNDERTALE'}, {id:'other',name:'Other game'}];
                if (channel === 'getGameInfo') return {name:'UNDERTALE'};
                if (channel === 'howManyMods') return count;
                if (channel === 'getInstallations') return [{index:0,name:'My & game',pid:'toby.undertale',valid:true,issues:[]}];
                if (channel === 'getSystemIndex') return 0;
                if (channel === 'isDevMode') return true;
                if (channel === 'setUniqueFlag' || channel === 'setInstallationCName') {
                    if (window.__failSave) throw new Error('Simulated disk write failure');
                    return true;
                }
                return false;
            }
        };
        window.electronAPI = window.deltamodBackend;
        window.communityAPI = {};
        document.documentElement.style.cssText = '--theme-color:rgb(205,68,81);--theme-color-hover:rgb(226,98,109);--theme-color-point2:rgba(205,68,81,.2);--theme-color-point3:rgba(205,68,81,.3);--theme-color-ink:white;--theme-color-hover-ink:white';

    }, {count, view});
    await page.addScriptTag({path:path.join(web,'modules/localization.js')});
    await page.evaluate(() => window.Localization.ready);
    await page.addScriptTag({path:path.join(web,'modules/frontend-refinements.js')});
    const markup = read(`views/${view}/index.html`);
    const css = markup.match(/STYLESHEET\[([^\]]+)\]/)?.[1];
    if (css) await page.addStyleTag({content:cssText(path.join(web, 'views', view, css + '.css'))});
    await page.evaluate(({markup, view}) => {
        const body = markup.replace(/^(?:JSL|NO-SIDEBAR|(?:STYLESHEET|TITLE|AUDIO|THEME-AUDIO-EXCLUDE)\[[^\]]*\])\s*$/gm, '');
        document.querySelector('.viewport').innerHTML = body.replace(/\$\$(.*?)\$\$/g, (_,key) => window.Localization.t(key,key));
        document.querySelector('#pageTitle').textContent = document.querySelector('.viewport h1')?.textContent || view;
        document.querySelectorAll('.sidebar-button').forEach(button => button.classList.toggle('active', button.dataset.page === view));
        window.Localization.apply(document.querySelector('.viewport'));
    }, {markup, view});
    await page.addScriptTag({path:path.join(web,`views/${view}/index.js`)});
    return page;
}
module.exports = {openView, read, web};
