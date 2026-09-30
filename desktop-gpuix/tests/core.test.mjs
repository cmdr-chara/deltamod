// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import path from 'node:path';
import { Bridge, startBridge, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES } from '../src/bridge.mjs';
import { AppModel, normalizeSnapshot, filterMods, normalizeShop } from '../src/model.mjs';
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
const rawSnapshot = () => ({ sourceAttached: true, installations: [], mods: [], themes: [], preferences: { themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true }, warnings: [] });
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
