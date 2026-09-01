// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');
const withoutThemeSceneEffects = source => source.replace(
    /\/\* theme-scene-effects:start[\s\S]*?theme-scene-effects:end \*\//,
    ''
);

describe('motion system', () => {
    it('uses shared timing tokens and respects reduced motion', () => {
        const baseCss = read('web/commons/base.css');
        const shellCss = read('web/index.css');

        expect(baseCss).toContain('--motion-fast: 140ms');
        expect(baseCss).toContain('--motion-standard: 180ms');
        expect(baseCss).toContain('--motion-ease-out: cubic-bezier(0.2, 0.8, 0.2, 1)');
        expect(shellCss).toContain('@media (prefers-reduced-motion: reduce)');
    });

    it('keeps navigation, alerts, and patching free of scale effects', () => {
        const interactiveSources = [
            'web/index.js',
            'web/index.css',
            'web/views/options/options.css',
            'web/views/locate/index.html',
            'web/views/patching/style.css',
            'web/haAlignments/Top.css',
            'web/haAlignments/Bottom.css',
            'web/haAlignments/Center.css'
        ].map(relativePath => {
            const source = read(relativePath);
            return relativePath === 'web/index.css'
                ? withoutThemeSceneEffects(source)
                : source;
        }).join('\n');

        expect(interactiveSources).not.toMatch(/\bscale(?:3d|X|Y)?\s*\(/i);
        expect(interactiveSources).not.toMatch(/\bscale\s*:/i);
        expect(interactiveSources).not.toMatch(/translate[XY]\([+-]?200px\)/i);
    });

    it('isolates decorative theme motion and disables it for reduced motion', () => {
        const shellCss = read('web/index.css');

        expect(shellCss).toContain('theme-scene-effects:start');
        expect(shellCss).toContain('theme-scene-effects:end');
        expect(shellCss).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.theme-transition-overlay[\s\S]*display: none !important/);
        expect(shellCss).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.theme-environment-sprite[\s\S]*animation: none !important/);
    });

    it('mounts section content without a navigation fade', () => {
        const shellScript = read('web/index.js');

        expect(shellScript).not.toContain('playOpacityEntry');
        expect(shellScript).toContain('const pageMarkupCache = new Map()');
        expect(shellScript).toContain('audio.play().catch(() => {})');
    });

    it('keeps the download animation two-dimensional and bounded', () => {
        const downloadModal = read('web/dlmodal/index.html');

        expect(downloadModal).not.toMatch(/rotate[XY]\(/);
        expect(downloadModal).not.toMatch(/scale\s*:/);
        expect(downloadModal).toContain('cancelAnimationFrame(canvas.animationFrame)');
        expect(downloadModal).toContain('if (elapsed < 1)');
        expect(downloadModal).toContain('prefers-reduced-motion: reduce');
    });

    it('uses a bounded Mod Shop skeleton and disables it for reduced motion', () => {
        const shopMarkup = read('web/views/gamebanana-browse/index.html');
        const shopCss = read('web/views/gamebanana-browse/indx.css');
        const shopScript = read('web/views/gamebanana-browse/index.js');

        expect(shopMarkup).toContain('shop-skeleton-row');
        expect(shopMarkup).toContain('<div class="scrollBottomDetector" aria-hidden="true"></div>');
        expect(shopMarkup).not.toMatch(/scrollBottomDetector[\s\S]{0,80}loadingBar/);
        expect(shopScript).toContain('function renderSourceLoading(table, rowCount = 4)');
        expect(shopScript).not.toContain("table.innerHTML = ''");
        expect(shopCss).toContain('animation: shop-skeleton-sweep 1.1s var(--motion-ease-out) infinite');
        expect(shopCss).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.shop-skeleton::after[\s\S]*display: none/);
    });
});
