// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { stageFrontend } = require('../stage-frontend');
const { inventory, audit } = require('./package-footprint.cjs');
const { launchOrder, validateConfig, traceCollector, runPair } = require('./paired-windows.cjs');
const installStartup = require('../../web/modules/desktop-startup');
const installAdapter = require('../../web/tauri-adapter');
const temporary = t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-performance-'));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    return root;
};
function write(root, file, data = 'fixture') {
    const destination = path.join(root, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, data);
    return destination;
}
function startupFixture(platform = 'Win32', installer = false) {
    const scope = new EventTarget();
    const scripts = [];
    const scheduled = new Map();
    const classes = new Set(['deltamod-route-pending']);
    const viewport = {};
    Object.assign(scope, {
        Event, navigator: { platform }, performance: { now: () => 10 },
        location: { replace: value => { scope.redirect = value; } },
        console: { warn() {}, error() {} },
        setTimeout(callback) { const id = Symbol(); scheduled.set(id, callback); return id; },
        clearTimeout(id) { scheduled.delete(id); },
        deltamodBackend: { invoke: async () => installer },
        document: {
            createElement: () => ({}),
            body: { appendChild: script => scripts.push(script) },
            documentElement: { classList: { remove: name => classes.delete(name) } },
            querySelector: () => viewport
        }
    });
    return { scope, scripts, scheduled, classes, viewport, api: installStartup(scope) };
}
const tick = () => new Promise(resolve => setImmediate(resolve));

test('frontend staging excludes duplicate media and development inputs without changing sources', t => {
    const root = temporary(t);
    write(root, 'web/index.html', '<html lang="en"><body></body></html>');
    write(root, 'web/boot/deltamod-boot.js');
    write(root, 'web/boot/deltamod-boot.css');
    write(root, 'web/themes/mus/track.mp3', Buffer.alloc(4096));
    write(root, 'web/themes/data/base.theme.json', '{}');
    write(root, 'web/themes/img/base.png');
    write(root, 'web/index.js', "assetUrl('app', `web/themes/${folder}/${file}`)");
    write(root, 'web/components/example.tsx');
    const report = stageFrontend(root);
    assert.equal(report.externalThemeMediaBytes, 4096);
    assert.equal(fs.existsSync(path.join(root, 'dist/frontend/themes/mus/track.mp3')), false);
    assert.equal(fs.existsSync(path.join(root, 'dist/frontend/themes/img/base.png')), true);
    assert.equal(fs.existsSync(path.join(root, 'dist/frontend/components')), false);
    assert.equal(fs.existsSync(path.join(root, 'web/themes/mus/track.mp3')), true);
    assert.doesNotMatch(fs.readFileSync(path.join(root, 'web/index.html'), 'utf8'), /external-theme-media/);
    assert.match(fs.readFileSync(path.join(root, 'dist/frontend/index.html'), 'utf8'), /data-deltamod-external-theme-media="true"/);
});

test('literal and dynamic direct media paths remain embedded as compatibility fallbacks', t => {
    const root = temporary(t);
    write(root, 'web/index.html', '<html lang="en"><audio src="themes/mus/direct.mp3"></audio></html>');
    write(root, 'web/boot/deltamod-boot.js');
    write(root, 'web/boot/deltamod-boot.css');
    write(root, 'web/index.js', 'const url = `./themes/video/${file}`;');
    write(root, 'web/themes/mus/direct.mp3');
    write(root, 'web/themes/video/movie.webm');
    write(root, 'web/themes/mus/unused.mp3', Buffer.alloc(32));
    const report = stageFrontend(root);
    assert.equal(report.externalThemeMediaBytes, 32);
    assert.equal(fs.existsSync(path.join(root, 'dist/frontend/themes/mus/direct.mp3')), true);
    assert.equal(fs.existsSync(path.join(root, 'dist/frontend/themes/video/movie.webm')), true);
});

test('inventory rejects links and enforces bounded traversal', t => {
    const root = temporary(t);
    write(root, 'files/a');
    write(root, 'files/b');
    assert.throws(() => inventory(path.join(root, 'files'), { maxFiles: 1 }), /bounds/);
    assert.throws(() => inventory(path.join(root, 'files'), { maxBytes: 1 }), /bounds/);
    fs.symlinkSync(path.join(root, 'files'), path.join(root, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
    assert.throws(() => inventory(path.join(root, 'linked')), /Linked/);
});

test('package audit distinguishes logical installed bytes, tool groups and installer download', async t => {
    const root = temporary(t);
    const data = Buffer.alloc(8192, 5);
    write(root, 'app/third-party/g3mtool/a.bin', data);
    write(root, 'app/themes/a.bin', data);
    write(root, 'app/themes/b.bin', Buffer.alloc(8192, 7));
    const installer = write(root, 'installer.exe', 'download');
    const result = await audit(path.join(root, 'app'), installer);
    assert.equal(result.installed.bytes, 24576);
    assert.equal(result.installer.bytes, 8);
    assert.equal(result.installed.categories.g3mtool, 8192);
    assert.equal(result.duplicates.length, 1);
    assert.equal(result.duplicates[0].redundantBytes, 8192);
});

test('startup fetches the core concurrently with ordered execution and defers optional UI', async () => {
    const { api, scope, scripts, scheduled, classes } = startupFixture();
    let ready = 0;
    scope.addEventListener('deltamod-route-ready', () => ready++);
    const result = api.start();
    assert.equal(api.start(), result);
    await tick();
    assert.equal(scripts.length, 6);
    assert.ok(scripts.every(script => script.async === false));
    assert.ok(classes.has('deltamod-route-pending'));
    assert.ok(!scripts.some(script => /linux|media-compat|updater-notice/.test(script.src)));
    for (const script of scripts) script.onload();
    await result;
    assert.equal(ready, 1);
    assert.equal(classes.has('deltamod-route-pending'), false);
    const [lazy] = scheduled.values();
    lazy();
    assert.equal(scripts.at(-1).src, './modules/updater-notice.js');
    scope.dispatchEvent(new Event('pagehide'));
    assert.equal(scheduled.size, 1); // Only the manually fired callback remains in this mock.
});

test('Linux compatibility remains ordered before the renderer', async () => {
    const { api, scope, scripts } = startupFixture('Linux x86_64');
    const result = api.start();
    await tick();
    assert.equal(scripts[0].src, './media-compat.js');
    assert.equal(scripts.at(-1).src, './linux-runtime-polish.js');
    scripts.forEach(script => script.onload());
    await result;
    scope.dispatchEvent(new Event('pagehide'));
});

test('installer route never starts community scripts', async () => {
    const { api, scope, scripts } = startupFixture('Win32', true);
    await api.start();
    assert.equal(scope.redirect, './installer/index.html');
    assert.equal(scripts.length, 0);
    scope.dispatchEvent(new Event('pagehide'));
});

test('failed startup does not initialize the renderer a second time', async () => {
    const { api, scope, scripts, viewport } = startupFixture();
    const result = api.start();
    const rejected = assert.rejects(result, /Unable to load/);
    await tick();
    scripts[0].onerror();
    await rejected;
    assert.equal(api.start(), result);
    assert.equal(scripts.length, 6);
    assert.match(viewport.textContent, /could not start/);
    scope.dispatchEvent(new Event('pagehide'));
});

test('startup diagnostics are bounded, immutable snapshots and not readiness substitutes', () => {
    const { api, scope } = startupFixture();
    const sink = [];
    api.record('ipc', 'loadedDeltarune', 0, 25);
    const snapshot = api.snapshot();
    snapshot.spans[0].name = 'mutated';
    api.benchmarkReady(value => sink.push(value));
    api.record('ipc', 'too-late', 0, 100);
    assert.equal(api.snapshot().spans[0].name, 'loadedDeltarune');
    assert.equal(api.snapshot().spans.length, 1);
    assert.match(sink[0], /DELTAMOD_STARTUP/);
    assert.ok(sink[0].length < 2048);
    scope.dispatchEvent(new Event('pagehide'));
});

function adapterFixture(href, external) {
    const scope = new EventTarget();
    const calls = [];
    Object.assign(scope, {
        location: { href }, URL,
        document: { documentElement: { dataset: { deltamodExternalThemeMedia: external ? 'true' : undefined } } },
        __TAURI__: { core: { invoke: async (...args) => { calls.push(args); return true; } }, event: { listen: async () => () => {} } }
    });
    installAdapter(scope);
    return { scope, calls, api: scope.deltamodBackend };
}

test('staged Windows media uses the scoped external protocol and source previews stay unchanged', () => {
    const windows = adapterFixture('http://tauri.localhost/index.html', true).api;
    assert.equal(windows.assetUrl('app', 'web/themes/mus/base.mp3'), 'http://themeprot.localhost/asset/mus/base.mp3');
    assert.equal(windows.assetUrl('app', 'web/themes/img/base.png'), 'http://tauri.localhost/themes/img/base.png');
    const unix = adapterFixture('tauri://localhost/index.html', true).api;
    assert.equal(unix.assetUrl('app', 'web/themes/video/chara.mp4'), 'themeprot://asset/video/chara.mp4');
    const source = adapterFixture('http://localhost:3000/web/index.html', false).api;
    assert.equal(source.assetUrl('app', 'web/themes/video/chara.mp4'), 'http://localhost:3000/web/themes/video/chara.mp4');
    for (const value of ['web/themes/../../secret.mp3', 'web/themes/%252e%252e/secret.mp3', 'C:\\secret.mp3']) {
        assert.throws(() => windows.assetUrl('app', value));
    }
});

test('diagnostic timing preserves native results and never records IPC arguments', async () => {
    const { scope, api, calls } = adapterFixture('http://tauri.localhost/index.html', true);
    const spans = [];
    scope.performance = { now: () => 3 };
    scope.DeltamodStartup = { record: (...args) => spans.push(args), benchmarkReady() {} };
    assert.equal(await api.invoke('getUniqueFlag', ['SECRET_PAYLOAD']), true);
    assert.equal(calls[0][1].data[0], 'SECRET_PAYLOAD');
    assert.ok(!JSON.stringify(spans).includes('SECRET_PAYLOAD'));
});

function pairFixture(t) {
    const root = temporary(t);
    const config = { outputDirectory: path.join(root, 'results'), seedDataRoot: path.join(root, 'seed') };
    fs.mkdirSync(config.seedDataRoot);
    for (const label of ['baseline', 'candidate']) {
        config[label] = { sourceRevision: (label === 'baseline' ? 'a' : 'b').repeat(40), artifactPath: path.join(root, label), executablePath: write(root, `${label}/app.exe`) };
    }
    return config;
}

test('paired capture alternates seven samples each after independent warmups', async t => {
    const config = pairFixture(t);
    const calls = [];
    const capture = {
        DEFAULT_PROTOCOL: { measuredLaunches: 7, warmupLaunches: 1 },
        collectEnvironment: async () => ({ processor: 'same-host' }),
        normalizeOptions: value => value,
        createReadinessFileProbe: () => 'real-marker-probe',
        runLaunch: async value => {
            assert.equal(value.options.seedDataRoot, config.seedDataRoot);
            assert.equal(value.probe, 'real-marker-probe');
            calls.push(value.label);
            return value.measured ? { readyMs: 123, peakWorkingSetBytes: 456 } : undefined;
        },
        summarizeSamples: samples => ({ count: samples.length }),
        inspectArtifact: () => ({ unpackedFileCount: 1, unpackedBytes: 7 }),
        writeImmutableJson: (file, data) => fs.writeFileSync(file, JSON.stringify(data), { flag: 'wx' })
    };
    await runPair(config, { capture, compare: (a, b) => {
        assert.equal(a.samples.length, 7);
        assert.equal(b.samples.length, 7);
        assert.equal(a.environment, b.environment);
        return { paired: true };
    } });
    assert.deepEqual(calls, launchOrder().map(item => item.label));
    assert.deepEqual(calls.slice(2, 6), ['baseline', 'candidate', 'candidate', 'baseline']);
    await assert.rejects(runPair(config, { capture, compare: () => ({}) }), /overwrite/);
});

test('pair validation rejects reused installations and escaped executables', t => {
    const config = pairFixture(t);
    assert.equal(validateConfig(config), config);
    config.candidate.executablePath = config.baseline.executablePath;
    assert.throws(() => validateConfig(config), /inside/);
});

test('trace collector handles chunk boundaries and ignores malformed or excess output', () => {
    const collector = traceCollector();
    collector.accept('[BENCHMARK] DELTAMOD_STAR');
    collector.accept('TUP {"phase":"renderer-ready"}\n');
    collector.accept('DELTAMOD_STARTUP not-json\n');
    collector.accept(('DELTAMOD_STARTUP {"phase":"boot-dismissed"}\n').repeat(10));
    assert.equal(collector.records.length, 4);
    assert.equal(collector.records[0].phase, 'renderer-ready');
});

function noticeFixture() {
    class Element extends EventTarget {
        constructor(tag) { super(); this.tag = tag; this.children = []; }
        append(...children) { for (const child of children) { this.children.push(child); child.parent = this; } }
        setAttribute(name, value) { this[name] = value; }
        removeAttribute(name) { delete this[name]; }
        remove() { this.parent.children = this.parent.children.filter(child => child !== this); }
    }
    const body = new Element('body');
    let receive;
    let resolve;
    const status = new Promise(done => { resolve = done; });
    const mount = require('../../web/modules/updater-notice');
    const controller = mount({ body, createElement: tag => new Element(tag) }, {
        status: () => status,
        onStatus: callback => { receive = callback; return () => {}; },
        onProgress: () => () => {}
    });
    return { body, controller, resolve, receive, panel: body.children[0] };
}

test('deferred updater UI restores current native state without overwriting newer events', async () => {
    const earlier = noticeFixture();
    earlier.resolve({ state: 'downloading' });
    await tick();
    assert.equal(earlier.panel.hidden, false);
    assert.match(earlier.panel.children[1].textContent, /Downloading/);
    earlier.controller.dispose();
    const raced = noticeFixture();
    raced.receive({ state: 'installed' });
    raced.resolve({ state: 'downloading' });
    await tick();
    assert.match(raced.panel.children[1].textContent, /installed/);
    raced.controller.dispose();
    assert.equal(raced.body.children.length, 0);
});

test('an incomplete paired capture retains its failure rather than writing a success result', async t => {
    const config = pairFixture(t);
    const capture = {
        DEFAULT_PROTOCOL: { measuredLaunches: 7 },
        collectEnvironment: async () => ({}),
        normalizeOptions: value => value,
        createReadinessFileProbe: () => () => false,
        runLaunch: async () => { throw new Error('readiness timeout'); },
        writeImmutableJson: (file, data) => fs.writeFileSync(file, JSON.stringify(data), { flag: 'wx' })
    };
    await assert.rejects(runPair(config, { capture, compare: () => { throw new Error('should not compare'); } }), /readiness timeout/);
    assert.ok(fs.existsSync(path.join(config.outputDirectory, 'pair-error.json')));
    assert.equal(fs.existsSync(path.join(config.outputDirectory, 'comparison.json')), false);
});
