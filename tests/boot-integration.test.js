const fs = require('fs');
const path = require('path');

const projectRoot = path.resolve(__dirname, '..');

describe('Deltamod boot screen integration', () => {
    test('ships the generated dependency-free boot bundle and stylesheet', () => {
        const bundle = path.join(projectRoot, 'web', 'boot', 'deltamod-boot.js');
        const stylesheet = path.join(projectRoot, 'web', 'boot', 'deltamod-boot.css');
        const build = fs.readFileSync(path.join(projectRoot, 'vite.boot.config.js'), 'utf8');
        const entry = fs.readFileSync(path.join(projectRoot, 'web', 'boot-native-entry.js'), 'utf8');
        expect(fs.existsSync(bundle)).toBe(true);
        expect(fs.statSync(bundle).size).toBeGreaterThan(1000);
        expect(fs.statSync(bundle).size).toBeLessThan(40000);
        expect(fs.existsSync(stylesheet)).toBe(true);
        expect(fs.statSync(stylesheet).size).toBeGreaterThan(1000);
        expect(build).toContain('web/boot-native-entry.js');
        expect(entry).not.toMatch(/from\s+["']react/);
    });

    test('mounts the overlay before the vanilla renderer starts', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const loader = fs.readFileSync(path.join(projectRoot, 'web', 'modules', 'desktop-startup.js'), 'utf8');
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'index.css'), 'utf8');
        expect(html).toContain('id="deltamod-boot-root"');
        expect(html).toContain('boot/deltamod-boot.css');
        expect(html).toContain('./modules/desktop-startup.js');
        const core = loader.slice(loader.indexOf('const core = ['));
        expect(core.indexOf('./boot/deltamod-boot.js')).toBeLessThan(core.indexOf('src="index.js"'));
        expect(css).toContain('#deltamod-boot-root:not([hidden]):not([data-dismissed="true"]) ~ .language-wheel-toggle');
        expect(css).toContain('body.deltamod-ui-entering > .language-wheel-toggle');
    });

    test('keeps the alert overlay empty and hidden until renderer-owned content exists', () => {
        const html = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'index.css'), 'utf8');
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        expect(html).toContain('<div class="alertMain" hidden>');
        expect(html).not.toContain('Example Alert');
        expect(css).toContain('.alertMain[hidden]');
        expect(renderer).toContain('alertMain.hidden = false;');
        expect(renderer).toContain('alertMain.hidden = true;');
    });

    test('connects theme and real initialization milestones to the boot API', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const boot = fs.readFileSync(path.join(projectRoot, 'web', 'modules', 'boot-native.mjs'), 'utf8');
        expect(renderer).toContain('window.DeltamodBoot?.setTheme');
        expect(renderer).toContain("bootProgress(0.03, 'Starting local runtime')");
        expect(renderer).toContain("bootProgress(0.9, 'Preparing file overlay')");
        expect(renderer).toContain('finishBoot();');
        expect(renderer).toContain("window.DeltamodBoot?.fail('Continuing')");
        expect(renderer).toContain("window.deltamodBackend.invoke('isCMode', [])");
        expect(renderer).toContain("invokeOptional('shouldGoIM', [], false)");
        expect(renderer).toContain("invokeOptional('executeArgumentCmd', [], null)");
        expect(renderer).toContain("assetUrl('app', `web/themes/${themePath}`)");
        expect(boot).toContain("doc.body.classList.add('deltamod-ui-entering')");
        expect(boot).toContain("theme.soulColor || theme.themeColor || '#ffffff'");
        expect(boot).toContain("section.style.setProperty('--db-soul-color', color)");
        expect(boot).toContain("doc.getElementById('theme-background-video')");
        expect(boot).not.toContain("createElement('video')");
    });

    test('holds the Chara boot lock until its vocal cue finishes', () => {
        const theme = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'web', 'themes', 'data', 'chara.theme.json'), 'utf8'
        ));
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const boot = fs.readFileSync(path.join(projectRoot, 'web', 'modules', 'boot-native.mjs'), 'utf8');
        expect(theme.bootSyncTime).toBeCloseTo(5.6, 2);
        expect(renderer).toContain('readyAtVideoTime: Number.isFinite(themeConfig.bootSyncTime)');
        expect(boot).toContain('activeVideo.currentTime >= cue');
    });
});
