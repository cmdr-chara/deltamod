const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const viewsRoot = path.join(root, 'web', 'views');
const themeRoot = path.join(root, 'web', 'themes');
const read = file => fs.readFileSync(file, 'utf8');
const viewNames = fs.readdirSync(viewsRoot, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && entry.name !== 'electron-tracer')
    .map(entry => entry.name)
    .sort();
const themeFiles = fs.readdirSync(path.join(themeRoot, 'data'))
    .filter(name => name.endsWith('.theme.json'))
    .sort();
const gameFiles = fs.readdirSync(path.join(root, 'games'))
    .filter(name => name.endsWith('.json'))
    .sort();

describe('renderer page smoke contracts', () => {
    it.each(viewNames)('%s has loadable markup and JavaScript', view => {
        const directory = path.join(viewsRoot, view);
        const html = read(path.join(directory, 'index.html'));
        const scriptPath = path.join(directory, 'index.js');

        expect(html.trim().length).toBeGreaterThan(0);
        expect(fs.existsSync(scriptPath), `${view}/index.js exists`).toBe(true);
        expect(() => new Function(read(scriptPath))).not.toThrow();

        for (const [, audioPath] of html.matchAll(/AUDIO\[([^\]]+)\]/g)) {
            if (audioPath.includes('/')) {
                expect(fs.existsSync(path.join(root, 'web', audioPath)), `${audioPath} exists`).toBe(true);
            }
        }
        for (const [, stylesheet] of html.matchAll(/STYLESHEET\[([^\]]+)\]/g)) {
            const candidates = [
                path.join(directory, `${stylesheet}.css`),
                path.join(directory, 'index.css'),
                path.join(directory, 'style.css'),
                path.join(directory, 'indx.css'),
                path.join(directory, 'main.css'),
                path.join(directory, 'options.css'),
                path.join(directory, 'themesel.css'),
                path.join(directory, 'credits.css'),
                path.join(directory, 'autoupdate.css')
            ];
            expect(candidates.some(fs.existsSync), `${view} stylesheet ${stylesheet} exists`).toBe(true);
        }
    });

    it('only navigates to existing views', () => {
        const known = new Set(viewNames);
        const missing = [];
        for (const view of viewNames) {
            const script = read(path.join(viewsRoot, view, 'index.js'));
            for (const [, target] of script.matchAll(/\bpage\(['"]([a-z0-9-]+)['"]/gi)) {
                if (!known.has(target)) missing.push(`${view} -> ${target}`);
            }
        }
        expect(missing).toEqual([]);
    });

    it('keeps inline page handlers enabled by the Tauri CSP', () => {
        const config = JSON.parse(read(path.join(root, 'src-tauri', 'tauri.conf.json')));
        expect(config.app.security.csp['script-src-attr']).toContain("'unsafe-inline'");
        expect(config.app.security.csp['style-src-attr']).toContain("'unsafe-inline'");
    });

    it('closes through the backend instead of clearing the WebView document', () => {
        const appMarkup = read(path.join(root, 'web', 'index.html'));
        const appScript = read(path.join(root, 'web', 'index.js'));
        expect(appMarkup).toContain('onclick="closeCommunity()"');
        expect(appMarkup).not.toContain('onclick="window.close()"');
        expect(appScript).toContain("invoke('quitCommunityForEasterEgg', [])");
    });

    it('updates theme and options language without reloading the current page', () => {
        const appScript = read(path.join(root, 'web', 'index.js'));
        const options = read(path.join(viewsRoot, 'options', 'index.js'));
        const localization = read(path.join(root, 'web', 'modules', 'localization.js'));
        const themeRefresh = appScript.slice(
            appScript.indexOf('async function themeRefresh'),
            appScript.indexOf('window.preloadAPI.onThemeChange')
        );
        expect(themeRefresh).not.toContain('page(pageN)');
        expect(themeRefresh).toContain('applyThemeStyles(theme)');
        expect(options).toContain("setLanguage(language.code, { refreshPage: false })");
        expect(localization).toContain('function applyKnownText(root = document)');
        expect(localization).toContain('async function setLanguage(code, { refreshPage = false } = {})');
        expect(localization).toContain('if (refreshPage && typeof window.page');
    });

    it('does not rerender an already active section when its navigation is clicked again', () => {
        const appScript = read(path.join(root, 'web', 'index.js'));
        const sidebarNavigation = appScript.slice(
            appScript.indexOf("var ribbon = document.querySelectorAll('.sidebar-button')"),
            appScript.indexOf('// Initialize Theme prior to initial page loads')
        );
        expect(sidebarNavigation).toContain('target === pageN');
        expect(sidebarNavigation).toContain('target === activePageNavigation');
        expect(sidebarNavigation).toContain('target === queuedPageNavigation?.target');

        const router = appScript.slice(
            appScript.indexOf('function page(name)'),
            appScript.indexOf('function schedulePageNavigationDrain')
        );
        expect(router).not.toContain('target === pageN');
    });

    it('localizes the live theme count without reloading the selector', () => {
        const themeSelector = read(path.join(viewsRoot, 'themesel', 'index.js'));
        const themeMarkup = read(path.join(viewsRoot, 'themesel', 'index.html'));
        const themeStyles = read(path.join(viewsRoot, 'themesel', 'themesel.css'));
        const localization = read(path.join(root, 'web', 'modules', 'localization.js'));
        expect(themeMarkup).not.toContain('THE TRUE NAME');
        expect(themeStyles).toMatch(/#theme-count\s*\{[^}]*text-align:\s*right;/s);
        expect(themeSelector).toContain("'theme_count'");
        expect(themeSelector).toContain("elisten(window, 'deltamod-language-change', updateDynamicTranslations)");
        expect(localization).toContain("theme_count: '{0} di {1} temi'");
        expect(localization).toContain("theme_count: '{0} z {1} motywów'");
    });
});

describe('built-in theme smoke contracts', () => {
    it.each(themeFiles)('%s has valid metadata and packaged assets', file => {
        const theme = JSON.parse(read(path.join(themeRoot, 'data', file)));
        expect(theme.id).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
        expect(theme.name).toEqual(expect.any(String));
        expect(theme.description).toEqual(expect.any(String));
        expect(theme.background).toEqual(expect.any(String));
        expect(theme.mainSong).toEqual(expect.any(String));
        expect(theme.musicTrack).toEqual(expect.any(String));
        expect(theme.color).toMatch(/^(?:rgb\(|#)/);
        expect(theme.soulColor).toMatch(/^(?:rgb\(|#)/);
        expect(fs.existsSync(path.join(themeRoot, 'img', theme.background)), `${theme.background} exists`).toBe(true);
        expect(fs.existsSync(path.join(themeRoot, 'mus', theme.mainSong)), `${theme.mainSong} exists`).toBe(true);
        if (theme.backgroundVideo) {
            expect(fs.existsSync(path.join(themeRoot, 'video', theme.backgroundVideo)), `${theme.backgroundVideo} exists`).toBe(true);
        }
    });

    it('has unique theme IDs and filenames', () => {
        const themes = themeFiles.map(file => JSON.parse(read(path.join(themeRoot, 'data', file))));
        expect(new Set(themes.map(theme => theme.id)).size).toBe(themes.length);
        for (const theme of themes) {
            expect(themeFiles).toContain(`${theme.id}.theme.json`);
        }
    });
});

describe('game catalogue smoke contracts', () => {
    it.each(gameFiles)('%s maps its catalogue providers and platform files', file => {
        const game = JSON.parse(read(path.join(root, 'games', file)));
        expect(game.id).toBe(file.replace(/\.json$/, ''));
        expect(game.name).toEqual(expect.any(String));
        if (game.platforms) {
            expect(Object.keys(game.platforms).length).toBeGreaterThan(0);
            for (const platform of Object.values(game.platforms)) {
                expect(platform.dataFiles?.length).toBeGreaterThan(0);
                expect(platform.patchLayout).toEqual(expect.any(String));
            }
        } else {
            expect(game.exeName).toEqual(expect.any(String));
        }
        if (game.gamebanana) expect(game.gamebanana.id).toBeGreaterThan(0);
        if (game.sources?.nexus) expect(game.sources.nexus.domain).toMatch(/^[a-z0-9-]+$/);
        if (game.sources?.moddb) expect(game.sources.moddb.slug).toMatch(/^[a-z0-9-]+$/);
    });
});
