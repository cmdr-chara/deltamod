// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-02.
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');

describe('Patch menu UI', () => {
    it('groups sorting and actions in one compact toolbar', () => {
        const markup = read('web/views/main/index.html');

        expect(markup).toContain('<header class="patch-toolbar">');
        expect(markup).toContain('class="patch-sort-control"');
        expect(markup).toContain('class="patch-actions"');
        expect(markup).toContain('id="importModBtn"');
        expect(markup).toContain('id="par"');
    });

    it('uses a themed accessible toggle instead of the native checkbox chrome', () => {
        const css = read('web/views/main/main.css');
        const script = read('web/views/main/index.js');

        expect(css).toContain('.patch-toggle-track::after');
        expect(css).toContain('.patch-toggle input:focus-visible + .patch-toggle-track');
        expect(css).not.toContain('border: 6px solid white');
        expect(script).toContain("toggleLabel.className = 'patch-toggle'");
        expect(script).toContain("enabled.setAttribute('aria-label', `Enable ${mod.name}`)");
        expect(script).toContain("modRow.classList.toggle('is-enabled', isEnabled)");
    });

    it('keeps Patch menu motion minimal and respects reduced-motion preferences', () => {
        const css = read('web/views/main/main.css');

        expect(css).not.toMatch(/\bscale(?:3d|X|Y)?\s*\(/i);
        expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.patch-toggle-track::after[\s\S]*transition: none/);
    });
});