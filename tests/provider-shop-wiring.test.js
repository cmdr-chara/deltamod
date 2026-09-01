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
        expect(renderer).toContain('offline: navigator.onLine === false');
        expect(renderer).toContain('browseGameBananaCatalog(furl)');
        expect(renderer).not.toContain("fetch(furl)");
        expect(renderer).toContain('Showing saved results because the live catalogue is unavailable.');
        expect(renderer).not.toContain('page(\'main\');\n        return;\n    }\n    let table');
    });

    it('has no dead Game Jolt or itch.io Mod Shop rendering branches', () => {
        expect(shell).not.toContain('ShopProvider::GameJolt');
        expect(shell).not.toContain('ShopProvider::Itch');
        expect(renderer).not.toContain("SHOP_PROVIDER === 'gamejolt'");
        expect(renderer).not.toContain("SHOP_PROVIDER === 'itch'");
    });
});
