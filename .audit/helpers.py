from pathlib import Path

def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)

write('scripts/lib/bounded-response.js',r'''// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';

/** Bound the actual stream, not just an optional Content-Length header. */
async function readBoundedResponse(response, maxBytes, label = 'Download') {
    if (!Number.isSafeInteger(maxBytes) || maxBytes < 1) throw new TypeError('Invalid download limit.');
    if (!response.ok) throw new Error(`${label} failed with HTTP ${response.status}`);
    const declared = response.headers.get('content-length');
    if (declared !== null && /^\d+$/.test(declared) && BigInt(declared) > BigInt(maxBytes)) {
        await response.body?.cancel().catch(() => {});
        throw new Error(`${label} exceeded its size limit.`);
    }
    if (!response.body) throw new Error(`${label} has no response body.`);
    const reader = response.body.getReader();
    const chunks = [];
    let total = 0;
    let block = null;
    let used = 0;
    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            if (value.byteLength > maxBytes - total) throw new Error(`${label} exceeded its size limit.`);
            let offset = 0;
            // Fixed-size blocks also bound bookkeeping when the server sends tiny chunks.
            while (offset < value.byteLength) {
                if (!block || used === block.length) {
                    block = Buffer.allocUnsafe(Math.min(64 * 1024, maxBytes - total));
                    chunks.push(block);
                    used = 0;
                }
                const count = Math.min(block.length - used, value.byteLength - offset);
                block.set(value.subarray(offset, offset + count), used);
                used += count;
                offset += count;
                total += count;
            }
        }
        if (!total) throw new Error(`${label} is empty.`);
        chunks[chunks.length - 1] = block.subarray(0, used);
        return Buffer.concat(chunks, total);
    } catch (error) {
        await reader.cancel().catch(() => {});
        throw error;
    } finally {
        reader.releaseLock();
    }
}
module.exports = { readBoundedResponse };
''')
write('scripts/lib/stage-sidecars.js',r'''// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { SUPPORTED_TARGETS } = require('./tauri-target');
const CRATES = Object.freeze(['hash-worker', 'security-worker', 'copy-worker', 'patch-plan-worker', 'patch-transaction-worker']);

function cargoOutputRoot(root, env = process.env) {
    return env.CARGO_TARGET_DIR ? path.resolve(root, env.CARGO_TARGET_DIR) : path.join(root, 'native', 'target');
}
function regularFile(io, file) {
    const stat = io.lstatSync(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size === 0) throw new Error(`Invalid sidecar file: ${file}`);
}
function stageSidecars({ sourceRoot, destination, target }, io = fs) {
    if (!SUPPORTED_TARGETS.has(target)) throw new Error('Unsupported sidecar target.');
    const extension = target.includes('windows') ? '.exe' : '';
    const entries = CRATES.map(crate => ({
        source: path.join(sourceRoot, target, 'release', `deltamod-${crate}${extension}`),
        name: `deltamod-${crate}-${target}${extension}`
    }));
    // A missing fifth binary must not destroy an existing complete bundle.
    for (const entry of entries) regularFile(io, entry.source);
    io.mkdirSync(destination, { recursive: true });
    if (io.lstatSync(destination).isSymbolicLink()) throw new Error('Sidecar destination is a link.');
    const lock = path.join(destination, `.stage-${target}.lock`);
    io.mkdirSync(lock); // An overlapping writer or unfinished recovery fails closed.
    const published = [];
    let preserveRecovery = false;
    try {
        for (const entry of entries) {
            entry.final = path.join(destination, entry.name);
            entry.staged = path.join(lock, entry.name);
            entry.previous = path.join(lock, `${entry.name}.previous`);
            try {
                regularFile(io, entry.final);
                io.copyFileSync(entry.final, entry.previous);
                entry.hadPrevious = true;
            } catch (error) {
                if (error.code !== 'ENOENT') throw error;
                entry.hadPrevious = false;
            }
            io.copyFileSync(entry.source, entry.staged);
            if (!extension) io.chmodSync(entry.staged, 0o755);
        }
        for (const entry of entries) {
            io.renameSync(entry.staged, entry.final);
            published.push(entry);
        }
    } catch (error) {
        for (const entry of published.reverse()) {
            try {
                if (entry.hadPrevious) io.renameSync(entry.previous, entry.final);
                else io.unlinkSync(entry.final);
            } catch {
                preserveRecovery = true;
            }
        }
        if (preserveRecovery) throw new Error(`Sidecar publication failed; recovery files retained at ${lock}`, { cause: error });
        throw error;
    } finally {
        if (!preserveRecovery) io.rmSync(lock, { recursive: true, force: true });
    }
    return entries.length;
}
module.exports = { CRATES, cargoOutputRoot, stageSidecars };
''')
write('scripts/stage-tauri-target.js',r'''const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { resolveTauriTarget } = require('./lib/tauri-target');
const { CRATES, cargoOutputRoot, stageSidecars } = require('./lib/stage-sidecars');

const root = path.resolve(__dirname, '..');
const target = resolveTauriTarget(process.argv[2]);
const workspace = path.join(root, 'native');
const binaries = path.join(root, 'src-tauri', 'binaries');
const cargo = process.env.CARGO || (process.platform === 'win32'
    ? path.join(process.env.USERPROFILE || '', '.cargo', 'bin', 'cargo.exe')
    : 'cargo');
const result = spawnSync(cargo, ['build', '--release', '--locked', '--target', target,
    '--manifest-path', path.join(workspace, 'Cargo.toml'),
    ...CRATES.flatMap(crate => ['--package', `deltamod-${crate}`])],
{ cwd: root, stdio: 'inherit', shell: false });
if (result.error) throw result.error;
if (result.signal || result.status === null) throw new Error(`Cargo did not complete: ${result.signal || 'unknown exit status'}`);
if (result.status !== 0) process.exit(result.status);
const count = stageSidecars({ sourceRoot: cargoOutputRoot(root), destination: binaries, target });
console.log(`Staged ${count} Tauri sidecars for ${target}.`);
''')
p=Path('scripts/stage-butler.js'); s=p.read_text()
s=s.replace("const { resolveTauriTarget } = require('./lib/tauri-target');", "const { resolveTauriTarget } = require('./lib/tauri-target');\nconst { readBoundedResponse } = require('./lib/bounded-response');")
a="  if (!response.ok) throw new Error(`butler acquisition failed with HTTP ${response.status}`);\n  const bytes = Buffer.from(await response.arrayBuffer());\n  if (!bytes.length || bytes.length > 64 * 1024 * 1024) throw new Error('butler archive exceeded its size limit.');"
assert a in s; s=s.replace(a,"  const bytes = await readBoundedResponse(response, 64 * 1024 * 1024, 'butler archive');")
a="    for (const entry of fs.readdirSync(extracted)) fs.copyFileSync(path.join(extracted, entry), path.join(destination, entry));"
assert a in s; s=s.replace(a,a+"\n    if (!executable.endsWith('.exe')) fs.chmodSync(path.join(destination, executable), 0o755);")
p.write_text(s)

write('scripts/tauri-parity/test/platform-staging.test.js',r'''// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { readBoundedResponse } = require('../../lib/bounded-response');
const { CRATES, cargoOutputRoot, stageSidecars } = require('../../lib/stage-sidecars');
const targets = ['x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin'];
function fixture(t, target = targets[1]) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-stage-'));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const sourceRoot = path.join(root, 'target');
    const destination = path.join(root, 'binaries');
    fs.mkdirSync(path.join(sourceRoot, target, 'release'), { recursive: true });
    const extension = target.includes('windows') ? '.exe' : '';
    const names = CRATES.map(crate => `deltamod-${crate}-${target}${extension}`);
    const sources = CRATES.map(crate => path.join(sourceRoot, target, 'release', `deltamod-${crate}${extension}`));
    sources.forEach((file, index) => fs.writeFileSync(file, `new-${index}`));
    return { sourceRoot, destination, target, names, sources };
}
for (const target of targets) test(`stages only the selected target: ${target}`, t => {
    const f = fixture(t, target);
    fs.mkdirSync(f.destination);
    fs.writeFileSync(path.join(f.destination, 'other-target'), 'preserved');
    assert.equal(stageSidecars(f), 5);
    f.names.forEach((name, index) => {
        const file = path.join(f.destination, name);
        assert.equal(fs.readFileSync(file, 'utf8'), `new-${index}`);
        if (process.platform !== 'win32' && !target.includes('windows')) assert.equal(fs.statSync(file).mode & 0o777, 0o755);
    });
    assert.equal(fs.readFileSync(path.join(f.destination, 'other-target'), 'utf8'), 'preserved');
});
test('missing source does not alter the existing staged set', t => {
    const f = fixture(t); stageSidecars(f);
    fs.unlinkSync(f.sources.at(-1));
    assert.throws(() => stageSidecars(f));
    f.names.forEach((name, index) => assert.equal(fs.readFileSync(path.join(f.destination, name), 'utf8'), `new-${index}`));
});
test('publication failure restores every already-published file', t => {
    const f = fixture(t); stageSidecars(f);
    f.sources.forEach(file => fs.writeFileSync(file, 'replacement'));
    let publishes = 0;
    const io = { ...fs, renameSync(from, to) {
        if (!from.endsWith('.previous') && ++publishes === 3) throw new Error('injected publish failure');
        fs.renameSync(from, to);
    } };
    assert.throws(() => stageSidecars(f, io), /injected publish failure/);
    f.names.forEach((name, index) => assert.equal(fs.readFileSync(path.join(f.destination, name), 'utf8'), `new-${index}`));
    assert.ok(!fs.existsSync(path.join(f.destination, `.stage-${f.target}.lock`)));
});
test('failed rollback preserves recovery files and refuses a new writer', t => {
    const f = fixture(t); stageSidecars(f);
    let calls = 0;
    const io = { ...fs, renameSync(from, to) {
        if (++calls > 1) throw new Error('injected unavailable destination');
        fs.renameSync(from, to);
    } };
    assert.throws(() => stageSidecars(f, io), /recovery files retained/);
    const lock = path.join(f.destination, `.stage-${f.target}.lock`);
    assert.ok(fs.existsSync(path.join(lock, `${f.names[0]}.previous`)));
    assert.throws(() => stageSidecars(f), { code: 'EEXIST' });
});
test('honors CARGO_TARGET_DIR without altering its meaning', () => {
    const root = path.resolve('repo');
    assert.equal(cargoOutputRoot(root, {}), path.join(root, 'native', 'target'));
    assert.equal(cargoOutputRoot(root, { CARGO_TARGET_DIR: '../shared' }), path.resolve(root, '../shared'));
});
test('rejects linked source binaries', t => {
    const f = fixture(t);
    fs.unlinkSync(f.sources[0]);
    try { fs.symlinkSync(f.sources[1], f.sources[0]); }
    catch (error) { if (error.code === 'EPERM') { t.skip('symlink permission unavailable'); return; } throw error; }
    assert.throws(() => stageSidecars(f), /Invalid sidecar/);
    assert.ok(!fs.existsSync(f.destination));
});
function response(chunks, headers = {}) {
    let cancelled = false;
    return { get cancelled() { return cancelled; }, value: new Response(new ReadableStream({
        start(controller) { for (const chunk of chunks) controller.enqueue(Uint8Array.from(chunk)); },
        cancel() { cancelled = true; }
    }), { headers }) };
}
test('rejects declared over-limit response before reading', async () => {
    const r = response([[1]], { 'content-length': '99999999999999999999999999' });
    await assert.rejects(readBoundedResponse(r.value, 8), /size limit/);
    assert.equal(r.cancelled, true);
});
test('rejects actual oversized stream even with a lying header and cancels it', async () => {
    const r = response([[1, 2], [3, 4, 5]], { 'content-length': '1' });
    await assert.rejects(readBoundedResponse(r.value, 4), /size limit/);
    assert.equal(r.cancelled, true);
});
test('preserves chunk contents and exact limit, including many tiny chunks', async () => {
    const expected = Buffer.alloc(65539, 97);
    const r = new Response(new ReadableStream({ start(controller) {
        for (const byte of expected) controller.enqueue(Uint8Array.of(byte));
        controller.close();
    } }));
    assert.deepEqual(await readBoundedResponse(r, expected.length), expected);
    assert.equal(r.body.locked, false);
});
test('empty responses, HTTP errors, and invalid limits fail explicitly', async () => {
    await assert.rejects(readBoundedResponse(new Response(''), 4), /empty/);
    await assert.rejects(readBoundedResponse(new Response('error', { status: 503 }), 4), /HTTP 503/);
    await assert.rejects(readBoundedResponse(new Response('x'), 0), TypeError);
    await assert.rejects(readBoundedResponse(new Response(null), 4), /no response body/);
});
''')
write('scripts/tauri-parity/test/tauri-only.test.js',r'''// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../../..');
const read = file => fs.readFileSync(path.join(root, file), 'utf8');
const json = file => JSON.parse(read(file));
function* files(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) yield* files(file);
        else if (/\.(?:js|cjs|mjs|html)$/.test(file)) yield file;
    }
}
test('Electron has no entry point, runtime dependency, lock entry, or packaging script', () => {
    const p = json('package.json');
    assert.equal(p.main, undefined); assert.equal(p.build, undefined);
    for (const name of Object.keys({ ...p.dependencies, ...p.devDependencies })) assert.doesNotMatch(name, /(?:^|\/)electron(?:$|-)/);
    assert.doesNotMatch(JSON.stringify(p.scripts), /electron|scripts\/legacy|node\/Runner/);
    for (const name of Object.keys(json('package-lock.json').packages)) assert.doesNotMatch(name, /(?:^|\/)electron(?:$|-)/);
});
test('retired runtime and publication paths are absent; benchmark history is retained', () => {
    for (const file of ['node/Runner.js', 'node/IPCHandlers.js', 'web/preload.js', 'web/dlmodal', 'web/views/electron-tracer', 'web/views/deleteall', 'scripts/legacy', '.github/workflows/release.yml', '.github/workflows/auto-release.yml', 'node/security/CredentialStorage.js']) {
        assert.ok(!fs.existsSync(path.join(root, file)), file);
    }
    assert.ok(fs.existsSync(path.join(root, 'benchmarks/desktop')));
    assert.ok(fs.existsSync(path.join(root, 'scripts/desktop-benchmark')));
});
test('active code does not import Electron or expose its renderer alias', () => {
    const imports = /(?:require\s*\(\s*|from\s*|import\s*\(\s*)['"](?:electron|electron-updater|electron-builder)(?:[/'"])/;
    for (const folder of ['web', 'node', 'scripts']) for (const file of files(path.join(root, folder))) {
        if (file.includes(`${path.sep}test${path.sep}`) || file.includes(`${path.sep}desktop-benchmark${path.sep}`)) continue;
        const source = fs.readFileSync(file, 'utf8');
        assert.doesNotMatch(source, imports, file);
        assert.doesNotMatch(source, /(?:window|root)\.electronAPI/, file);
    }
});
test('all public native channels are implemented; retired controls are not exposed', () => {
    const { buildParity, assertParity } = require('../lib/parity');
    const report = buildParity({ contractPath: path.join(root, 'web/tauri-adapter.js'), rustPath: path.join(root, 'src-tauri/src/main.rs') });
    assertParity(report);
    assert.equal(report.counts.rustUnsupported, 0);
    assert.equal(report.counts.rendererInvoke, report.counts.rustImplemented);
    const adapter = read('web/tauri-adapter.js');
    for (const channel of ['rebootDev','createInstallLink','undertaleModTool:openInstallation','gamebanana_downloadAllInCollection','npsCallback','initialize']) {
        assert.ok(!adapter.includes(`'${channel}'`), channel);
        for (const file of ['web/views/options/index.js','web/views/installmanager/index.js','web/views/collections/index.js','web/views/patching/index.js']) assert.ok(!read(file).includes(`'${channel}'`), file);
    }
});
test('Unix bundle configuration cannot inherit NSIS or Windows controller payloads', () => {
    const base = json('src-tauri/tauri.conf.json');
    const linux = json('src-tauri/tauri.linux.conf.json');
    const mac = json('src-tauri/tauri.macos.conf.json');
    const windows = json('src-tauri/tauri.windows.conf.json');
    assert.deepEqual(linux.bundle.targets, ['deb']);
    assert.equal(linux.bundle.createUpdaterArtifacts, false);
    assert.deepEqual(mac.bundle.targets, ['app', 'dmg']);
    assert.equal(mac.bundle.createUpdaterArtifacts, true);
    assert.doesNotMatch(JSON.stringify(base.bundle.resources), /cmodeutil\.exe/);
    assert.match(JSON.stringify(windows.bundle.resources), /cmodeutil\.exe/);
});
test('shared Node reference modules have no dangling relative imports', () => {
    for (const file of files(path.join(root, 'node'))) {
        const source = fs.readFileSync(file, 'utf8');
        for (const match of source.matchAll(/require\(\s*['"](\.[^'"]+)['"]\s*\)/g)) {
            const target = path.resolve(path.dirname(file), match[1]);
            assert.ok([target, `${target}.js`, `${target}.json`, path.join(target, 'index.js')].some(candidate => fs.existsSync(candidate)), `${file}: ${match[1]}`);
        }
    }
});
''')
print('Bounded streaming and target-scoped sidecar staging implemented with behavioral regression tests.')
