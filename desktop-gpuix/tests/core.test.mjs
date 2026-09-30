// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import path from 'node:path';
import { Bridge, startBridge, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES } from '../src/bridge.mjs';
import { AppModel, normalizeSnapshot, filterMods, normalizeShop, queryMods, DEFAULT_LIBRARY, shortcutFor } from '../src/model.mjs';
import { parseOptions, openModPage } from '../src/options.mjs';
import { parsePs, sumTree, summarize } from '../scripts/processes.mjs';

function child() {
  const value = new EventEmitter();
  value.stdin = new PassThrough(); value.stdout = new PassThrough(); value.stderr = new PassThrough();
  value.exitCode = null; value.signalCode = null; value.kills = 0;
  value.kill = () => { value.kills++; value.signalCode = 'SIGTERM'; };
  return value;
}
function bridgeFixture(t, options) {
  const process = child();
  const bridge = new Bridge(process, options);
  const frames = [];
  process.stdin.on('data', value => frames.push(JSON.parse(String(value))));
  t.after(() => bridge.close());
  return { process, bridge, frames, reply: (id, result) => process.stdout.write(JSON.stringify({ v: 1, id, ok: true, result }) + '\n') };
}
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const rawSnapshot = () => ({ sourceAttached: true, readOnly: true, selectedInstallationId: null, games: [{ id: 'toby.deltarune', name: 'DELTARUNE', gamebanana: true }], installations: [], mods: [], themes: [], preferences: { themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true }, warnings: [] });
function modelFixture(t) {
  const calls = [];
  const model = new AppModel({ request: (command, args) => { const pending = deferred(); calls.push({ command, args, ...pending }); return pending.promise; }, close() {} });
  t.after(() => model.dispose());
  return { model, calls };
}

test('stdio sends a versioned bounded command, not a shell or generic invoke', async t => {
  const { bridge, frames, reply } = bridgeFixture(t);
  const result = bridge.request('snapshot');
  assert.deepEqual(frames, [{ v: 1, id: 1, command: 'snapshot', args: {} }]);
  reply(1, { actual: true });
  assert.deepEqual(await result, { actual: true });
  await assert.rejects(bridge.request('deleteAllData'), /Unsupported/);
  await assert.rejects(bridge.request('snapshot', []), /Object/);
  await assert.rejects(bridge.request('profile.attach', { path: 'x'.repeat(MAX_REQUEST_BYTES) }), /too large/);
});
test('stdio handles fragmented UTF-8 and multiple responses', async t => {
  const { bridge, process } = bridgeFixture(t);
  const first = bridge.request('snapshot');
  const second = bridge.request('hello');
  const bytes = Buffer.from(JSON.stringify({ v: 1, id: 1, ok: true, result: '雪' }) + '\n' + JSON.stringify({ v: 1, id: 2, ok: true, result: 'second' }) + '\n');
  const split = bytes.indexOf(Buffer.from('雪')) + 1;
  process.stdout.write(bytes.subarray(0, split));
  process.stdout.write(bytes.subarray(split));
  assert.equal(await first, '雪'); assert.equal(await second, 'second');
});
test('incompatible frames reject all requests and terminate the owned backend', async t => {
  const { bridge, process } = bridgeFixture(t);
  const failed = assert.rejects(bridge.request('snapshot'), /Incompatible/);
  process.stdout.write('{"v":99,"id":1,"ok":true}\n');
  await failed;
  assert.equal(process.kills, 1);
  bridge.close(); assert.equal(process.kills, 1);
  await assert.rejects(bridge.request('snapshot'), /not connected/);
});
test('unexpected IDs and oversized unterminated frames fail closed', async t => {
  const first = bridgeFixture(t);
  const rejected = assert.rejects(first.bridge.request('snapshot'), /Unexpected/);
  first.reply(99, {}); await rejected;
  const second = bridgeFixture(t);
  const huge = assert.rejects(second.bridge.request('hello'), /limit/);
  second.process.stdout.write(Buffer.alloc(MAX_RESPONSE_BYTES + 1, 120)); await huge;
});
test('timeouts do not silently retry requests with unknown completion', async t => {
  const { bridge, process, frames } = bridgeFixture(t, { timeoutMs: 15 });
  await assert.rejects(bridge.request('preferences.set', {}), /timed out/);
  assert.equal(frames.length, 1); assert.equal(process.kills, 1);
});
test('native errors and backpressure are surfaced without optimistic success', async t => {
  const { bridge, process } = bridgeFixture(t, { maxPending: 1 });
  const request = assert.rejects(bridge.request('snapshot'), /cannot read/);
  await assert.rejects(bridge.request('hello'), /busy/);
  process.stdout.write('{"v":1,"id":1,"ok":false,"error":"cannot read"}\n');
  await request; assert.equal(bridge.closed, false);
});
test('native process launch uses argument arrays with independent data roots', t => {
  const process = child(); let call;
  const folder = path.resolve('path with spaces');
  const bridge = startBridge({ executable: path.join(folder, 'host'), stateRoot: path.join(folder, 'preview'), resourcesRoot: folder, sourceProfile: path.join(folder, 'source') }, (...args) => { call = args; return process; });
  t.after(() => bridge.close());
  assert.equal(call[2].shell, false);
  assert.deepEqual(call[1], ['--state-root', path.join(folder, 'preview'), '--resources-root', folder, '--source-profile', path.join(folder, 'source')]);
});
test('old snapshot responses cannot overwrite newer data', async t => {
  const { model, calls } = modelFixture(t);
  const first = model.refresh(); const second = model.refresh();
  calls[1].resolve({ ...rawSnapshot(), themes: [{ id: 'new', name: 'New', accent: '#112233' }] });
  await second;
  calls[0].resolve(rawSnapshot()); await first;
  assert.equal(model.state.snapshot.themes[0].name, 'New');
});
test('profile attachment failure leaves the last real snapshot intact', async t => {
  const { model, calls } = modelFixture(t);
  model.update({ snapshot: normalizeSnapshot(rawSnapshot()) });
  const before = model.state.snapshot;
  const result = model.attachProfile('/missing');
  calls[0].reject(new Error('Source unavailable')); await result;
  assert.equal(model.state.snapshot, before); assert.match(model.state.error, /unavailable/);
});
test('preferences are saved only after native acknowledgement', async t => {
  const { model, calls } = modelFixture(t);
  model.update({ snapshot: normalizeSnapshot(rawSnapshot()) });
  const before = model.state.snapshot.preferences;
  const result = model.savePreferences({ accent: '#123456' });
  assert.equal(model.state.snapshot.preferences, before); assert.equal(model.state.saving, true);
  calls[0].resolve({ ...before, accent: '#123456' }); await result;
  assert.equal(model.state.snapshot.preferences.accent, '#123456'); assert.equal(model.state.saving, false);
});
test('read-only capabilities are negotiated before loading the profile', async t => {
  const { model, calls } = modelFixture(t);
  const started = model.initialize();
  assert.equal(calls[0].command, 'hello');
  calls[0].resolve({ protocol: 2, capabilities: ['snapshot'], readOnly: true });
  await assert.rejects(started, /incompatible/); assert.equal(calls.length, 1);
});
test('old Mod Shop searches cannot replace a newer search', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(rawSnapshot()));
  const first = model.browse('old'); const second = model.browse('new');
  calls[1].resolve({ items: [{ id: '2', name: 'New', url: 'https://gamebanana.com/mods/2' }], hasMore: false }); await second;
  calls[0].resolve({ items: [{ id: '1', name: 'Old', url: 'https://gamebanana.com/mods/1' }], hasMore: true }); await first;
  assert.equal(model.state.shop.items[0].name, 'New'); assert.equal(model.state.shop.query, 'new');
});
test('unknown states are not invented and untrusted URLs never reach the opener', async () => {
  const snapshot = normalizeSnapshot({ ...rawSnapshot(), mods: [{ id: '1', name: 'Library mod', format: 'legacy' }] });
  assert.equal(snapshot.mods[0].enabled, null);
  assert.equal(filterMods(snapshot.mods, 'library').length, 1);
  assert.equal(filterMods(snapshot.mods, '', true).length, 0);
  assert.deepEqual(normalizeShop({ items: [{ name: 'Bad', url: 'file:///secret' }] }).items, []);
  for (const url of ['https://evil.test/mods/1', 'https://gamebanana.com.evil.test/mods/1', 'https://gamebanana.com/mods/1?run=x', 'javascript:alert(1)']) {
    await assert.rejects(openModPage(url), /not allowed/);
  }
});
test('disposed models cannot publish late native results', async t => {
  const { model, calls } = modelFixture(t); let notifications = 0;
  model.subscribe(() => notifications++);
  const result = model.refresh(); model.dispose();
  const count = notifications; calls[0].resolve(rawSnapshot()); await result;
  assert.equal(notifications, count); assert.equal(model.state.snapshot, null);
});
test('CLI keeps state separate from resources and rejects ambiguous flags', () => {
  const options = parseOptions(['--no-focus', '--reduce-motion', '--opaque', '--source-profile', '/profile']);
  assert.equal(options.focus, false); assert.equal(options.reducedMotion, true); assert.equal(options.opaque, true);
  assert.notEqual(options.stateRoot, options.sourceProfile);
  assert.ok(options.executable.endsWith(path.join('desktop-gpuix', 'native', 'target', 'release', `deltamod-gpuix-host${process.platform === 'win32' ? '.exe' : ''}`)));
  assert.throws(() => parseOptions(['--state-root']), /incomplete/);
  assert.throws(() => parseOptions(['--no-focus', '--no-focus']), /Duplicate/);
});
test('memory accounting includes the native host and excludes unrelated processes', () => {
  const records = parsePs(' 10 1 100\n 11 10 50\n 12 11 25\n 20 1 999\n');
  assert.deepEqual(sumTree(records, 10), { bytes: 175 * 1024, count: 3 });
  assert.throws(() => sumTree(records, 30), /Missing/);
  assert.throws(() => parsePs('x 1 2'), /Malformed/);
  assert.throws(() => sumTree([...records, records[0]], 10), /duplicate/);
});
test('all seven samples, including the slowest, stay in the summary', () => {
  const result = summarize([2, 3, 2.5, 2.1, 2.4, 5.51, 2.3].map(ready => ({ ready })), 'ready');
  assert.deepEqual(result, { minimum: 2, median: 2.4, p95NearestRank: 5.51 });
  assert.throws(() => summarize([{ ready: 1 }], 'ready'), /Seven/);
});

function workspaceSnapshot(selected = '0') {
  return { ...rawSnapshot(), selectedInstallationId: selected,
    games: [
      { id: 'toby.deltarune', name: 'DELTARUNE', gamebanana: true },
      { id: 'toby.undertale', name: 'UNDERTALE', gamebanana: true },
      { id: 'local.unsupported', name: 'Local game', gamebanana: false },
    ],
    installations: [
      { id: '0', name: 'Deltarune', gameId: 'toby.deltarune', path: '', current: true, selected: selected === '0' },
      { id: '1', name: 'Undertale', gameId: 'toby.undertale', path: '', current: false, selected: selected === '1' },
      { id: '2', name: 'Other game', gameId: 'local.unsupported', path: '', current: false, selected: selected === '2' },
    ],
    mods: [{ id: 'first', name: 'Existing mod', description: '', format: 'runtime', enabled: true }]
  };
}

test('session selection changes the preview and shop context, not Tauri current flags', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  assert.equal(model.state.shop.gameId, 'toby.deltarune');
  const selection = model.selectInstallation('1');
  assert.deepEqual(calls[0].args, { id: '1' });
  assert.equal(calls[0].command, 'installation.select');
  assert.equal(model.state.snapshot.selectedInstallationId, '0');
  calls[0].resolve(workspaceSnapshot('1'));
  assert.equal(await selection, true);
  assert.equal(model.state.snapshot.selectedInstallationId, '1');
  assert.equal(model.state.shop.gameId, 'toby.undertale');
  assert.equal(model.state.snapshot.installations[0].current, true);
  assert.equal(model.state.snapshot.installations[1].current, false);
});

test('disconnect clears profile data and filters only after acknowledgement', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  model.setLibrary({ query: 'private search', status: 'enabled' });
  const pending = model.detachProfile();
  assert.equal(model.state.snapshot.sourceAttached, true);
  assert.equal(calls[0].command, 'profile.detach');
  calls[0].resolve({ ...rawSnapshot(), sourceAttached: false });
  assert.equal(await pending, true);
  assert.equal(model.state.snapshot.sourceAttached, false);
  assert.equal(model.state.snapshot.mods.length, 0);
  assert.equal(model.state.snapshot.installations.length, 0);
  assert.deepEqual(model.state.library, DEFAULT_LIBRARY);
  assert.equal(model.state.profileEpoch, 1);
});

test('profile replacement invalidates outstanding shop data from the previous profile', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  const browse = model.browse('old profile search');
  const attach = model.attachProfile('/another-profile');
  calls[1].resolve(workspaceSnapshot('1'));
  await attach;
  calls[0].resolve({ items: [{ id: '1', name: 'Stale', url: 'https://gamebanana.com/mods/1' }], hasMore: true });
  await browse;
  assert.equal(model.state.shop.gameId, 'toby.undertale');
  assert.equal(model.state.shop.query, '');
  assert.equal(model.state.shop.items.length, 0);
  assert.equal(model.state.shop.status, 'idle');
});

test('profile operations cannot race a refresh or preference write', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  const attached = model.attachProfile('/profile');
  await assert.rejects(model.refresh(), /busy/);
  await model.savePreferences({ accent: '#123456' });
  assert.equal(await model.selectInstallation('1'), false);
  assert.equal(calls.length, 1);
  calls[0].resolve(workspaceSnapshot()); await attached;
  const saving = model.savePreferences({ accent: '#123456' });
  await assert.rejects(model.refresh(), /busy/);
  assert.equal(await model.detachProfile(), false);
  calls[1].resolve({ ...model.state.snapshot.preferences, accent: '#123456' }); await saving;
  assert.equal(model.state.snapshot.preferences.accent, '#123456');
});

test('failed session commands retain the last acknowledged selection', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot('1')));
  const result = model.selectInstallation('2');
  calls[0].reject(new Error('Installation removed'));
  assert.equal(await result, false);
  assert.equal(model.state.snapshot.selectedInstallationId, '1');
  assert.match(model.state.error, /removed/);
  assert.equal(model.state.profileEpoch, 0);
});

test('wrong native acknowledgements cannot report detach or selection success', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  const selected = model.selectInstallation('1');
  calls[0].resolve(workspaceSnapshot()); assert.equal(await selected, false);
  assert.match(model.state.error, /acknowledged/);
  const detached = model.detachProfile();
  calls[1].resolve(workspaceSnapshot()); assert.equal(await detached, false);
  assert.match(model.state.error, /not detached/);
});

test('game selection is allowlisted and transmitted to the Rust shop request', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  model.setShopGame('../outside'); assert.equal(model.state.shop.gameId, 'toby.deltarune');
  model.setShopGame('toby.undertale');
  const result = model.browse('snow', 3);
  assert.deepEqual(calls[0].args, { query: 'snow', page: 3, gameId: 'toby.undertale' });
  calls[0].resolve({ items: [], hasMore: false }); await result;
  assert.equal(model.state.shop.gameId, 'toby.undertale');
});

test('unsupported selected games cannot silently fall back to DELTARUNE', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot('2')));
  assert.equal(model.state.shop.gameId, '');
  await model.browse('anything');
  assert.equal(calls.length, 0);
  assert.match(model.state.shop.error, /Choose a game/);
  model.setShopGame('toby.undertale');
  assert.equal(model.state.shop.gameId, 'toby.undertale');
});

test('switching games clears previous pages and ignores their late responses', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  const old = model.browse('old');
  model.setShopGame('toby.undertale');
  calls[0].resolve({ items: [{ id: '1', url: 'https://gamebanana.com/mods/1' }], hasMore: true }); await old;
  assert.equal(model.state.shop.gameId, 'toby.undertale');
  assert.deepEqual(model.state.shop.items, []);
  assert.equal(model.state.shop.page, 1);
});

test('shop results are not stale during a request, an error or the terminal page', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  const first = model.browse('first');
  calls[0].resolve({ items: [{ id: '1', url: 'https://gamebanana.com/mods/1' }], hasMore: true }); await first;
  const second = model.browse('second'); assert.deepEqual(model.state.shop.items, []);
  calls[1].reject(new Error('Network failed')); await second;
  assert.deepEqual(model.state.shop.items, []); assert.equal(model.state.shop.hasMore, false);
  const last = model.browse('last', 100);
  calls[2].resolve({ items: [], hasMore: true }); await last;
  assert.equal(model.state.shop.hasMore, false);
});

test('library filters distinguish disabled from unknown and sort without mutating source', () => {
  const mods = [
    { id: '2', name: 'Mod 10', description: 'snow', enabled: null, format: 'legacy packet' },
    { id: '1', name: 'Mod 2', description: '', enabled: true, format: 'runtime' },
    { id: '3', name: 'Alpha', description: '', enabled: false, format: 'runtime' },
  ];
  const before = JSON.stringify(mods);
  assert.deepEqual(queryMods(mods, { ...DEFAULT_LIBRARY, status: 'unknown' }).map(item => item.id), ['2']);
  assert.deepEqual(queryMods(mods, { ...DEFAULT_LIBRARY, status: 'disabled' }).map(item => item.id), ['3']);
  assert.deepEqual(queryMods(mods, { ...DEFAULT_LIBRARY, format: 'runtime', sort: 'name-desc' }).map(item => item.id), ['1', '3']);
  assert.deepEqual(queryMods(mods, { ...DEFAULT_LIBRARY, sort: 'enabled-first' }).map(item => item.id), ['1', '3', '2']);
  assert.equal(queryMods(mods, { ...DEFAULT_LIBRARY, query: 'SNOW' })[0].id, '2');
  assert.equal(JSON.stringify(mods), before);
});

test('library filters persist across navigation while history is bounded and branchable', t => {
  const { model } = modelFixture(t);
  model.setLibrary({ query: 'remember', sort: 'name-desc' });
  model.navigate('library'); model.navigate('themes'); model.goBack();
  assert.equal(model.state.route, 'library'); assert.equal(model.state.library.query, 'remember');
  model.goForward(); assert.equal(model.state.route, 'themes');
  model.goBack(); model.navigate('settings'); assert.deepEqual(model.state.history.forward, []);
  const before = model.state.history.back.length; model.navigate('settings'); assert.equal(model.state.history.back.length, before);
  for (let i = 0; i < 100; i++) model.navigate(i % 2 ? 'home' : 'library');
  assert.equal(model.state.history.back.length, 32);
  model.setLibrary({ sort: 'invalid' }); assert.equal(model.state.library.sort, 'name-desc');
});

test('malformed snapshots and contradictory selections fail closed', () => {
  for (const patch of [
    { readOnly: false }, { installations: [null] }, { games: [{ id: '../secret' }] },
    { installations: [{ id: '0' }, { id: '0' }] }, { selectedInstallationId: 'missing' },
    { sourceAttached: false }, { mods: Array(501).fill({}) },
  ]) assert.throws(() => normalizeSnapshot({ ...workspaceSnapshot(), ...patch }));
  assert.throws(() => normalizeShop({ items: [null] }));
});

test('native shortcuts preserve platform conventions and reject repeat/AltGr chords', () => {
  assert.deepEqual(shortcutFor({ key: '2', modifiers: { ctrl: true } }, 'win32'), { action: 'navigate', route: 'library' });
  assert.deepEqual(shortcutFor({ key: 'f', modifiers: { cmd: true } }, 'darwin'), { action: 'search' });
  assert.deepEqual(shortcutFor({ key: 'o', modifiers: { ctrl: true, shift: true } }, 'linux'), { action: 'attach' });
  assert.deepEqual(shortcutFor({ key: 'left', modifiers: { alt: true } }, 'win32'), { action: 'back' });
  assert.deepEqual(shortcutFor({ key: 'tab', modifiers: { shift: true } }, 'darwin'), { action: 'tab-previous' });
  assert.equal(shortcutFor({ key: 'r', isHeld: true, modifiers: { ctrl: true } }, 'win32'), null);
  assert.equal(shortcutFor({ key: '2', modifiers: { ctrl: true, alt: true } }, 'win32'), null);
  assert.equal(shortcutFor({ key: 'f', modifiers: { ctrl: true } }, 'darwin'), null);
  assert.equal(shortcutFor({ key: 'r' }, 'win32'), null);
});

test('shortcut subscribers are cleaned up and disposed models make no requests', async t => {
  const { model, calls } = modelFixture(t); let seen = 0;
  const off = model.onShortcut(() => seen++);
  model.dispatchShortcut({ action: 'help' }); off(); model.dispatchShortcut({ action: 'help' });
  assert.equal(seen, 1);
  model.onShortcut(() => seen++); model.dispose(); model.dispatchShortcut({ action: 'help' });
  await model.browse('ignored'); await model.savePreferences({});
  assert.equal(await model.attachProfile('/ignored'), false);
  assert.equal(seen, 1); assert.equal(calls.length, 0);
});

test('new native capabilities must all be negotiated before a snapshot is requested', async t => {
  const { model, calls } = modelFixture(t);
  const result = model.initialize();
  calls[0].resolve({ protocol: 1, workspaceVersion: 2, readOnly: true,
    capabilities: ['snapshot', 'profile.attach', 'profile.detach', 'installation.select', 'shop.browse', 'preferences.set'] });
  await Promise.resolve();
  assert.equal(calls[1].command, 'snapshot');
  calls[1].resolve(rawSnapshot()); await result;
  assert.equal(model.state.shop.gameId, 'toby.deltarune');
});

test('native state changes are published only after the busy gate is released', async t => {
  const { model, calls } = modelFixture(t);
  model.acceptSnapshot(normalizeSnapshot(workspaceSnapshot()));
  let followup;
  const off = model.subscribe(() => {
    if (!model.state.loading && model.state.profileEpoch === 1 && !followup) {
      off(); followup = model.browse('followup');
    }
  });
  const result = model.selectInstallation('1'); calls[0].resolve(workspaceSnapshot('1')); await result;
  assert.equal(calls[1].command, 'shop.browse');
  calls[1].resolve({ items: [], hasMore: false }); await followup;
});
