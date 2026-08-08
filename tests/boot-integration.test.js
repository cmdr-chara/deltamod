const fs = require('fs');
const path = require('path');

const projectRoot = path.resolve(__dirname, '..');

describe('Deltamod boot screen integration', () => {
    test('ships the generated React bundle and stylesheet', () => {
        const bundle = path.join(projectRoot, 'web', 'boot', 'deltamod-boot.js');
        const stylesheet = path.join(projectRoot, 'web', 'boot', 'deltamod-boot.css');

        expect(fs.existsSync(bundle)).toBe(true);
        expect(fs.statSync(bundle).size).toBeGreaterThan(10000);
        expect(fs.existsSync(stylesheet)).toBe(true);
        expect(fs.statSync(stylesheet).size).toBeGreaterThan(1000);
    });

    test('mounts the overlay before the vanilla renderer starts', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'index.css'), 'utf8');
        expect(html).toContain('id="deltamod-boot-root"');
        expect(html).toContain('boot/deltamod-boot.css');
        expect(html).toContain('./boot/deltamod-boot.js');
        expect(html.indexOf('./boot/deltamod-boot.js')).toBeLessThan(html.indexOf('src="index.js"'));
        expect(css).toContain('#deltamod-boot-root:not([hidden]):not([data-dismissed="true"]) ~ .language-wheel-toggle');
        expect(css).toContain('body.deltamod-ui-entering > .language-wheel-toggle');
    });

    test('connects theme and real initialization milestones to the boot API', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const bootEntry = fs.readFileSync(path.join(projectRoot, 'web', 'boot-entry.tsx'), 'utf8');
        const bootScreen = fs.readFileSync(path.join(projectRoot, 'web', 'components', 'DeltamodBootScreen.tsx'), 'utf8');
        expect(renderer).toContain('window.DeltamodBoot?.setTheme');
        expect(renderer).toContain("bootProgress(0.03, 'Starting local runtime')");
        expect(renderer).toContain("bootProgress(0.9, 'Preparing file overlay')");
        expect(renderer).toContain('finishBoot();');
        expect(renderer).toContain("window.DeltamodBoot?.fail('Continuing')");
        expect(bootEntry).toContain('document.body.classList.add("deltamod-ui-entering")');
        expect(bootEntry).toContain('if (!state.themeReady)');
        expect(bootEntry).toContain('const accentColor = theme.soulColor || theme.themeColor;');
        expect(bootEntry).toContain('state.themeColor = accentColor;');
        expect(bootEntry).toContain('state.soulColor = accentColor;');
        expect(bootScreen).toContain('interactive={false}');
        expect(bootScreen).toContain('effectsContext.drawImage(');
        expect(bootScreen).toContain('data-video={backgroundVideo ? "on" : "off"}');
    });

    test('holds the Chara boot lock until its vocal cue finishes', () => {
        const theme = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'web', 'themes', 'data', 'chara.theme.json'),
            'utf8'
        ));
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const bootSource = fs.readFileSync(path.join(projectRoot, 'web', 'components', 'DeltamodBootScreen.tsx'), 'utf8');

        expect(theme.bootSyncTime).toBeCloseTo(5.6, 2);
        expect(renderer).toContain('readyAtVideoTime: Number.isFinite(themeConfig.bootSyncTime)');
        expect(bootSource).toContain('syncVideo.currentTime >= cueTime');
    });
});
