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
        const bootEntry = fs.readFileSync(path.join(projectRoot, 'web', 'boot-entry.tsx'), 'utf8');
        const bootScreen = fs.readFileSync(path.join(projectRoot, 'web', 'components', 'DeltamodBootScreen.tsx'), 'utf8');
        expect(renderer).toContain('window.DeltamodBoot?.setTheme');
        expect(renderer).toContain("bootProgress(0.03, 'Starting local runtime')");
        expect(renderer).toContain("bootProgress(0.9, 'Preparing file overlay')");
        expect(renderer).toContain('finishBoot();');
        expect(renderer).toContain("window.DeltamodBoot?.fail('Continuing')");
        expect(renderer).toContain("window.deltamodBackend.invoke('isCMode', [])");
        expect(renderer).toContain("invokeOptional('shouldGoIM', [], false)");
        expect(renderer).toContain("invokeOptional('executeArgumentCmd', [], null)");
        expect(renderer).toContain("assetUrl('app', `web/themes/${themePath}`)");
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

describe('production boot performance lifecycle', () => {
    function bootFixture() {
        const vm = require('node:vm');
        const ts = require('typescript');
        const source = fs.readFileSync(path.join(projectRoot, 'web', 'boot-entry.tsx'), 'utf8');
        const compiled = ts.transpileModule(source, {
            compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX }
        }).outputText;
        const classes = new Set();
        const timers = [];
        const renders = [];
        let unmounts = 0;
        const host = {
            dataset: {}, hidden: true,
            setAttribute() {}, removeAttribute() {}, replaceChildren() {}
        };
        const root = {
            render(value) { renders.push(value); },
            unmount() { unmounts += 1; }
        };
        const window = {
            setTimeout(callback, delay) { timers.push({ callback, delay }); return timers.length; },
            clearTimeout() {}
        };
        vm.runInNewContext(compiled, {
            exports: {}, window,
            document: {
                getElementById: () => host,
                body: { classList: { add: name => classes.add(name), remove: name => classes.delete(name) } }
            },
            require(name) {
                if (name === 'react-dom/client') return { createRoot: () => root };
                if (name === 'react/jsx-runtime') return { jsx: (type, props) => ({ type, props }) };
                if (name === './components/DeltamodBootScreen') return { default: 'BootScreen' };
                throw new Error(`Unexpected boot dependency: ${name}`);
            }
        });
        return { api: window.DeltamodBoot, host, timers, renders, classes, unmounts: () => unmounts };
    }

    test('removes the production time floor without discarding explicit theme cues', () => {
        const assert = require('node:assert/strict');
        const fixture = bootFixture();
        fixture.api.setTheme({ themeColor: '#ffffff', readyAtVideoTime: 5.6 });
        const props = fixture.renders.at(-1).props;
        assert.equal(props.minimumDuration, 0);
        assert.equal(props.readyAtVideoTime, 5.6);
        assert.equal(props.readyVideoElementId, 'theme-background-video');
        assert.equal(props.autoPlay, true);
        props.onReady();
        assert.equal(fixture.timers.length, 0);
        assert.equal(fixture.host.dataset.dismissed, undefined);
    });

    test('dismisses once after real completion and releases the React root', () => {
        const assert = require('node:assert/strict');
        const fixture = bootFixture();
        fixture.api.setTheme({ themeColor: '#ffffff' });
        fixture.api.finish();
        const props = fixture.renders.at(-1).props;
        assert.equal(props.progress, 1);
        props.onReady();
        props.onReady();
        assert.equal(fixture.timers.length, 1);
        assert.equal(fixture.host.dataset.dismissed, 'true');
        assert.equal(fixture.classes.has('deltamod-boot-active'), false);
        fixture.timers[0].callback();
        assert.equal(fixture.unmounts(), 1);
        assert.equal(fixture.host.hidden, true);
    });

    test('ignores late notifications after dismissal instead of rendering an unmounted root', () => {
        const assert = require('node:assert/strict');
        const fixture = bootFixture();
        fixture.api.setTheme({ themeColor: '#ffffff' });
        fixture.api.finish();
        fixture.renders.at(-1).props.onReady();
        fixture.timers[0].callback();
        const renderCount = fixture.renders.length;
        fixture.api.fail('Late notification');
        fixture.api.setProgress(0.5);
        fixture.api.setTheme({ themeColor: '#000000' });
        fixture.api.finish();
        assert.equal(fixture.renders.length, renderCount);
        assert.equal(fixture.timers.length, 1);
    });
});
