const fs = require('node:fs');
const path = require('node:path');
const UI = require('../web/modules/workspace');
const Icons = require('../web/modules/icons');
const read = value => fs.readFileSync(path.join(__dirname, '..', value), 'utf8');

describe('desktop workspace utilities and route contracts', () => {
    it('preserves meaningful progress and treats unknown values as indeterminate', () => {
        expect(UI.percent(0)).toBe(0);
        expect(UI.percent('42.5')).toBe(42.5);
        expect(UI.percent(101)).toBe(100);
        for (const input of [undefined, null, false, true, '', '   ', -1, Infinity, NaN, {}, []]) {
            expect(UI.percent(input)).toBe(null);
        }
        const bar = { removeAttribute: vi.fn() };
        UI.setProgress(bar, 62);
        expect(bar).toMatchObject({ max: 100, value: 62 });
        UI.setProgress(bar, undefined);
        expect(bar.removeAttribute).toHaveBeenCalledWith('value');
    });
    it('normalizes local search without discarding Japanese or accented names', () => {
        expect(UI.normalizeQuery('  Étoile MOD  ')).toBe('etoile mod');
        expect(UI.normalizeQuery('勇気')).toBe('勇気');
        expect(UI.normalizeQuery(null)).toBe('');
    });
    it('renders utility icons from fixed local geometry, never untrusted markup', () => {
        expect(Icons.markup('add_box')).toContain('<svg');
        expect(Icons.markup('download', '18px')).toContain('width:18px');
        expect(Icons.markup('<img onerror=attack>', '1px;background:url(https://evil)')).not.toContain('attack');
        expect(Icons.markup('check', 'small')).toContain('width:14px');
    });
    it('loads the actual workspace stylesheet and keeps every original navigation destination', () => {
        const html = read('web/index.html');
        expect(html).toContain('href="styles/deltamod-revamp.css"');
        expect(html).toContain('src="./modules/startup.js"');
        for (const name of ['main', 'allmods', 'options', 'installmanager', 'gamebanana-browse', 'collections', 'credits']) {
            expect(html).toContain(`data-page="${name}"`);
        }
        expect(html).not.toContain('fonts.googleapis.com/css2');
    });
    it('retains the full theme workshop instead of replacing it with a styling stub', () => {
        const html = read('web/views/themesel/index.html');
        for (const id of ['theme-import-form', 'theme-import-name', 'theme-import-description', 'theme-import-color', 'theme-import-include-music', 'theme-import-icon-preview', 'cancel-theme-import', 'create-theme']) {
            expect(html).toContain(`id="${id}"`);
        }
        expect(read('web/index.js')).toContain("'allmods-v2': './views/main/main.css'");
    });
    it('keeps patch output as bounded text, not executable HTML', () => {
        const script = read('web/views/patching/index.js');
        expect(script).toContain('line.textContent = message');
        expect(script).toContain('log.childElementCount > 200');
        expect(script).toContain('requestAnimationFrame(flush)');
        expect(script).toContain('nextButton.hidden = false');
        expect(script).not.toContain('innerHTML +=');
    });
});
