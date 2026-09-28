// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { describe, expect, it } = globalThis;

const root = join(__dirname, '..');
const shell = readFileSync(join(root, 'src-tauri', 'src', 'channels', 'mods.rs'), 'utf8');
const renderer = readFileSync(
    join(root, 'web', 'views', 'gamebanana-browse', 'index.js'),
    'utf8'
);

describe('provider Mod Shop shell wiring', () => {
    it('advertises only providers with an in-app catalogue', () => {
        const providerList = shell.slice(
            shell.indexOf('"modSources:getProviders"'),
            shell.indexOf('"modSources:browse"')
        );
        expect(providerList).toContain('"id":"gamebanana"');
        expect(providerList).toContain('"id":"nexus"');
        expect(providerList).toContain('"id":"moddb"');
        expect(providerList).not.toContain('"id":"gamejolt"');
        expect(providerList).not.toContain('"id":"itch"');
    });

    it('keeps remaining provider labels provider-specific', () => {
        expect(renderer).toContain("moddb: 'ModDB'");
        expect(renderer).toContain("nexus: 'Nexus Mods'");
        expect(renderer).not.toContain("SHOP_PROVIDER === 'moddb' ? 'ModDB' : 'Nexus Mods'");
    });

    it('uses the Rust catalogue cache and structured offline fallback', () => {
        expect(shell).toContain('browse_with_cache(');
        expect(shell).toContain('ProviderCatalogCache::request_key');
        expect(shell).toContain('normalized_provider_error');
        expect(renderer).not.toContain('navigator.onLine');
        expect(renderer).toContain('gameId: SHOP_GAME_ID');
        expect(renderer).toContain('fetchGameBananaCatalogDirect');
        expect(renderer).toContain('browseGameBananaCatalog(furl)');
        expect(renderer).not.toContain("fetch(furl)");
        expect(renderer).toContain('Showing saved results because the live catalogue is unavailable.');
        expect(renderer).not.toContain('page(\'main\');\n        return;\n    }\n    let table');
    });

    it('lets the shop choose a game independently from the active installation', () => {
        const markup = readFileSync(
            join(root, 'web', 'views', 'gamebanana-browse', 'index.html'),
            'utf8'
        );
        expect(markup).toContain('id="modGameSelect"');
        expect(renderer).toContain("invoke('getAvailableGames', [])");
        expect(renderer).toContain("localStorage.setItem('modShopGameId'");
        expect(renderer).toContain("'Mod Shop could not be loaded'");
        expect(renderer).toContain("'GameBanana could not be loaded'");
    });

    it('has no dead Game Jolt or itch.io Mod Shop rendering branches', () => {
        expect(shell).not.toContain('ShopProvider::GameJolt');
        expect(shell).not.toContain('ShopProvider::Itch');
        expect(renderer).not.toContain("SHOP_PROVIDER === 'gamejolt'");
        expect(renderer).not.toContain("SHOP_PROVIDER === 'itch'");
    });
});

// Exercise the actual page action without executing catalogue fetch/startup code.
function downloadHarness(outcome) {
    const { runInNewContext } = require('node:vm');
    const phases = [];
    const icons = [];
    const alerts = [];
    const buttons = [{ disabled: false }, { disabled: true }];
    const button = {
        disabled: false,
        setAttribute() {}, removeAttribute() {},
        style: { setProperty() {}, removeProperty() {} },
        classList: { add() {}, remove() {} }
    };
    const state = { current: true };
    const window = { _onClosePage: [], currentPageStack: { qms: {} } };
    window.deltamodBackend = { invoke: async (channel, args) => {
        expect(channel).toBe('dlmodURL');
        window.currentPageStack.qms[args[1]]({ progress: 42, downloaded: 42, total: 100 });
        if (outcome instanceof Error) throw outcome;
        return outcome;
    } };
    const action = renderer.slice(renderer.indexOf('async function dlmod('), renderer.indexOf('window.currentPageStack.dlmod ='));
    expect(action).toContain('finally');
    const dlmod = runInNewContext('let gameBananaDownloadActive = false; let pageActive = true; ' + action + '; dlmod', {
        window, document: { querySelectorAll: () => buttons },
        isCurrentShopPage: () => state.current,
        setDownloadButtonIcon: (button, icon) => icons.push(icon),
        updateModDownloadStatus: status => phases.push(status.phase),
        htmlAlert: async (...args) => alerts.push(args)
    });
    return { dlmod, window, button, buttons, state, phases, icons, alerts };
}

describe('mod download completion and retry', () => {
    it('reports success only after an affirmative native import result', async () => {
        const h = downloadHarness(true);
        expect(await h.dlmod('https://gamebanana.com/mmdl/1', h.button, 1, 'Mod')).toBe(true);
        expect(h.phases.at(-1)).toBe('complete');
        expect(h.icons).toContain('done_outline');
        expect(h.button.disabled).toBe(true);
        expect(h.buttons.map(b => b.disabled)).toEqual([false, true]);
        expect(Object.keys(h.window.currentPageStack.qms)).toEqual([]);
        expect(h.window._onClosePage).toEqual([]);
    });
    it('keeps cancellation or an existing-copy decision retryable instead of claiming success', async () => {
        const h = downloadHarness(false);
        for (let attempt = 0; attempt < 2; attempt++) {
            expect(await h.dlmod('https://gamebanana.com/mmdl/1', h.button, 1, 'Mod')).toBe(false);
        }
        expect(h.phases).not.toContain('complete');
        expect(h.phases.at(-1)).toBe('cancelled');
        expect(h.icons).not.toContain('done_outline');
        expect(h.button.disabled).toBe(false);
        expect(h.alerts).toEqual([]);
        expect(h.buttons.map(b => b.disabled)).toEqual([false, true]);
        expect(Object.keys(h.window.currentPageStack.qms)).toEqual([]);
    });
    it('shows errors and malformed acknowledgements without disabling retry', async () => {
        for (const outcome of [null, undefined, new Error('Archive rejected')]) {
            const h = downloadHarness(outcome);
            expect(await h.dlmod('https://gamebanana.com/mmdl/1', h.button, 1, 'Mod')).toBe(false);
            expect(h.phases.at(-1)).toBe('failed');
            expect(h.phases).not.toContain('complete');
            expect(h.alerts).toHaveLength(1);
            expect(h.button.disabled).toBe(false);
        }
    });
    it('allows progress for buttonless requests and ignores late page acknowledgements', async () => {
        const h = downloadHarness(true);
        const pending = h.dlmod('https://gamebanana.com/mmdl/1', null, 1, 'Mod');
        h.state.current = false;
        expect(await pending).toBe(true);
        expect(h.phases).not.toContain('complete');
        expect(Object.keys(h.window.currentPageStack.qms)).toEqual([]);
        expect(h.buttons.map(b => b.disabled)).toEqual([false, true]);
    });
});
