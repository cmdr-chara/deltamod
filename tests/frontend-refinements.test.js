// Copyright © 2026 cmdr-chara. Licensed under the EUPL 1.2.
const fs = require('node:fs');
const path = require('node:path');
const ui = require('../web/modules/frontend-refinements');
const read = file => fs.readFileSync(path.join(__dirname, '..', file), 'utf8');

describe('Original frontend refinement', () => {
    it('searches all author names, package IDs and versions without case sensitivity', () => {
        const mod = { name: 'Forest', author: ['Chara', 'Second'], version: '2.1', packageID: 'org.forest' };
        expect(ui.matches(mod, ' second  2.1 ')).toBe(true);
        expect(ui.matches(mod, 'ORG.forest')).toBe(true);
        expect(ui.matches(mod, 'Forest missing')).toBe(false);
        expect(ui.matches({}, '')).toBe(true);
        expect(ui.matches({ name: 'ＦＯＲＥＳＴ' }, 'forest')).toBe(true);
    });
    it('sorts numeric names, sizes, and either author representation predictably', () => {
        const mods = [
            { name: 'Mod 10', author: 'Beta', size: 10 },
            { name: 'Mod 2', author: ['Alpha'], size: 2 },
            { name: 'Mod 1', author: ['Alpha'], size: 2 }
        ];
        for (const order of ['asc', 'size-asc', 'author']) {
            expect([...mods].sort((a,b) => ui.compareMods(a,b,order)).map(mod => mod.name))
                .toEqual(['Mod 1','Mod 2','Mod 10']);
        }
        expect([...mods].sort((a,b) => ui.compareMods(a,b,'desc'))[0].name).toBe('Mod 10');
        expect(ui.compareMods({name:'A',size:null},{name:'B',size:0},'size-desc')).toBeLessThan(0);
    });
    it('retains the original identity assets, sidebar, typography and language wheel', () => {
        const html = read('web/index.html');
        const css = read('web/commons/base.css');
        expect(html).toContain('class="dmodicon-ring"');
        expect(html).toContain('class="language-wheel"');
        expect(html).toContain('sbar/main.png');
        expect(css).toContain('font-family: "Pixel"');
        expect(css).toContain('backdrop-filter: blur(var(--panel-blur))');
        expect(html).not.toContain('sidebar-brand');
        expect(html).not.toContain('deltamod-revamp.css');
        expect(html).toContain('gicons/sheet.css');
        expect(html).not.toContain('href="https://fonts.googleapis.com');
    });
    it('includes every new label in all eight existing localization catalogs', () => {
        const catalog = read('web/modules/localization.js');
        for (const key of ['refine_search_mods','refine_search_hint','refine_clear','refine_mod_count',
            'refine_no_matches','refine_saving','refine_saved','refine_save_failed','refine_progress',
            'refine_patch_complete','refine_patch_log']) {
            expect(catalog.match(new RegExp(`${key}:`, 'g'))).toHaveLength(8);
        }
    });
    it('does not reload native lists on sort/filter changes or poll suggestions', () => {
        const helper = read('web/modules/frontend-refinements.js');
        expect(helper).not.toContain("invoke('getModList'");
        const shop = read('web/views/gamebanana-browse/index.js');
        expect(shop).not.toContain('let sval = 0');
        expect(shop).toContain('encodeURIComponent(query)');
        expect(shop).toContain('request.signal');
        const patch = read('web/views/patching/index.js');
        expect(patch).not.toContain('innerHTML +=');
        expect(patch).toContain('line.textContent = message');
    });
});
