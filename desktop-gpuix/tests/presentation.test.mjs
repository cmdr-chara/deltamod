// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import path from 'node:path';
import { AppModel, normalizeSnapshot } from '../src/model.mjs';
import { DesktopFeatures, interfacePreferences, normalizeDetail, normalizeTheme, plainDescription } from '../src/features.mjs';
import { parseIntent } from '../src/deep-links.mjs';
import { parseOptions } from '../src/options.mjs';
import { dictionaries, placeholders, translate, formatBytes } from '../src/locales.mjs';
import { Bridge } from '../src/bridge.mjs';

const preferences = { schemaVersion: 1, locale: 'en', themeImages: false };
function snapshot() {
  return normalizeSnapshot({ readOnly: true, sourceAttached: false, selectedInstallationId: null,
    installations: [], mods: [], warnings: [],
    games: [{ id: 'toby.deltarune', name: 'DELTARUNE', gamebanana: true }, { id: 'toby.undertale', name: 'UNDERTALE', gamebanana: true }],
    themes: [{ id: 'base', name: 'Base', accent: '#112233' }, { id: 'chara', name: 'Chara', accent: '#223344' }],
    preferences: { themeId: 'base', accent: '#112233', reducedMotion: true, opaque: true } });
}
const detail = id => ({ id, name: 'Fixture', author: 'Creator', game: 'DELTARUNE', description: '<p>Hello &amp; welcome</p>', url: `https://gamebanana.com/mods/${id}`, files: [], hasContentRatings: false });
function fixture(t) {
  const calls = [];
  const bridge = { close() {}, request(command, args = {}) {
    return new Promise((resolve, reject) => calls.push({ command, args, resolve, reject }));
  } };
  const model = new AppModel(bridge);
  model.acceptSnapshot(snapshot());
  const features = new DesktopFeatures(model, bridge);
  t.after(() => { features.dispose(); model.dispose(); });
  return { calls, model, features };
}

test('English and Italian cover every message with matching named placeholders', () => {
  assert.equal(Object.keys(dictionaries.en).length, 170);
  assert.deepEqual(Object.keys(dictionaries.it).sort(), Object.keys(dictionaries.en).sort());
  for (const key of Object.keys(dictionaries.en)) {
    assert.ok(dictionaries.it[key].trim());
    assert.deepEqual(placeholders(dictionaries.it[key]), placeholders(dictionaries.en[key]), key);
  }
});
test('translation falls back without evaluating user names as templates or markup', () => {
  assert.equal(translate('it-IT', 'Settings'), 'Impostazioni');
  assert.equal(translate('unknown', 'Settings'), 'Settings');
  assert.equal(translate('it', 'By {author}', { author: '<img>{game}</img>' }), 'Di <img>{game}</img>');
  assert.equal(translate('it', 'By {author}'), 'Di {author}');
  assert.equal(translate('it', 'Unknown diagnostic'), 'Unknown diagnostic');
  assert.equal(translate('it', 'toString'), 'toString');
});
test('file sizes distinguish zero from missing data and use locale decimals', () => {
  assert.equal(formatBytes('en', 0), '0 B');
  assert.equal(formatBytes('it', 1536), '1,5 KiB');
  assert.equal(formatBytes('en', 1536), '1.5 KiB');
  for (const invalid of [null, undefined, NaN, -1, Infinity, 0.5]) assert.equal(formatBytes('en', invalid), 'Unknown size');
});
test('preview URLs yield only bounded read-only navigation intents', () => {
  assert.deepEqual(parseIntent('https://gamebanana.com/mods/42'), { kind: 'mod', id: '42' });
  assert.deepEqual(parseIntent('deltamod-gpuix-preview://screen?route=themes'), { kind: 'screen', route: 'themes' });
  assert.deepEqual(parseIntent('deltamod-gpuix-preview://browse?game=toby.undertale&q=snow%20mod'), { kind: 'browse', gameId: 'toby.undertale', query: 'snow mod' });
  assert.deepEqual(parseIntent('deltamod-gpuix-preview://mod?id=42'), { kind: 'mod', id: '42' });
});
test('links reject privileged schemes, credentials, traversal, duplicate and unknown parameters', () => {
  for (const raw of [
    'deltamod-community://install/42', 'file:///secret', 'javascript:alert(1)',
    'https://user:secret@gamebanana.com/mods/42', 'https://gamebanana.com.evil.test/mods/42',
    'https://gamebanana.com/mods/42?download=1', 'https://gamebanana.com/mods/42#x',
    'https://gamebanana.com/mods/01', 'https://gamebanana.com/mods/9007199254740992',
    'deltamod-gpuix-preview://browse?game=../outside', 'deltamod-gpuix-preview://screen?route=home&route=shop',
    'deltamod-gpuix-preview://screen?route=home&path=/source',
    'deltamod-gpuix-preview://browse?game=toby.deltarune&q=%00',
    'deltamod-gpuix-preview://browse?game=toby.deltarune&q=%zz',
    'deltamod-gpuix-preview://browse?game=toby.deltarune&q=' + 'x'.repeat(129),
  ]) assert.throws(() => parseIntent(raw), /links.invalid/, raw);
});
test('CLI preserves the URL as an intent and cannot mix it into benchmark launches', () => {
  const raw = 'https://gamebanana.com/mods/42';
  assert.equal(parseOptions(['--open', raw]).openLink, raw);
  assert.throws(() => parseOptions(['--open']), /links.invalid/);
  assert.throws(() => parseOptions(['--open', raw, '--open', raw]), /Duplicate/);
  assert.throws(() => parseOptions(['--open', raw, '--benchmark-file', '/tmp/ready']), /benchmark/);
});
test('interface settings are versioned and committed only after native acknowledgement', async t => {
  const { calls, features } = fixture(t);
  const loading = features.initialize();
  assert.equal(calls[0].command, 'ui.preferences.get');
  calls[0].resolve(preferences); await loading;
  const saving = features.savePreferences({ locale: 'it' });
  assert.equal(features.state.preferences.locale, 'en');
  assert.equal(features.state.saving, true);
  assert.equal(await features.savePreferences({ themeImages: true }), false);
  calls[1].resolve({ ...preferences, locale: 'it' });
  assert.equal(await saving, true);
  assert.equal(features.state.preferences.locale, 'it');
  assert.equal(features.state.saving, false);
});
test('failed or contradictory preference writes keep the last saved language', async t => {
  const { calls, features } = fixture(t);
  const saving = features.savePreferences({ locale: 'it' });
  calls[0].resolve(preferences);
  assert.equal(await saving, false);
  assert.equal(features.state.preferences.locale, 'en');
  assert.equal(features.state.error, 'features.notSaved');
  const retry = features.savePreferences({ locale: 'it' });
  calls[1].reject(new Error('disk full')); assert.equal(await retry, false);
  assert.equal(features.state.preferences.locale, 'en');
  assert.throws(() => interfacePreferences({ ...preferences, locale: '../it' }), /badPreferences/);
});
test('theme images are opt-in and duplicate requests do not accumulate', async t => {
  const { calls, features } = fixture(t);
  assert.equal(await features.loadTheme('base'), false); assert.equal(calls.length, 0);
  const save = features.savePreferences({ themeImages: true }); calls[0].resolve({ ...preferences, themeImages: true }); await save;
  const load = features.loadTheme('base');
  assert.equal(await features.loadTheme('base'), true); assert.equal(calls.length, 2);
  calls[1].resolve({ themeId: 'base', imagePath: path.resolve('bundled/base.png'), hasVideo: false, hasAudio: false });
  assert.equal(await load, true);
  assert.equal(features.state.theme.status, 'ready');
  assert.equal(await features.loadTheme('../secret'), false);
});
test('disabling images invalidates a pending image response', async t => {
  const { calls, features } = fixture(t);
  features.update({ preferences: { ...preferences, themeImages: true } });
  const load = features.loadTheme('base');
  const save = features.savePreferences({ themeImages: false });
  calls[1].resolve(preferences); await save;
  calls[0].resolve({ themeId: 'base', imagePath: path.resolve('bundled/base.png') });
  assert.equal(await load, false); assert.equal(features.state.theme.value, null);
});
test('theme responses must match the selected ID and contain only an absolute raster path', () => {
  for (const imagePath of ['https://evil.test/pic.png', '../pic.png', path.resolve('pic.svg'), 'bad\0.png']) {
    assert.throws(() => normalizeTheme({ themeId: 'base', imagePath }, 'base'), /badTheme/);
  }
  assert.throws(() => normalizeTheme({ themeId: 'other', imagePath: null }, 'base'), /badTheme/);
  assert.equal(normalizeTheme({ themeId: 'base', imagePath: null }, 'base').imagePath, null);
});
test('detail responses discard download URLs and decode descriptions as text', () => {
  const source = { ...detail('42'), files: [{ id: '7', name: 'mod.zip', version: '1', bytes: 0, downloadUrl: 'https://example.test/?token=SECRET' }] };
  const result = normalizeDetail(source, '42');
  assert.equal(result.description, 'Hello & welcome');
  assert.equal(result.files[0].bytes, 0);
  assert.ok(!JSON.stringify(result).includes('SECRET'));
  assert.throws(() => normalizeDetail(source, '43'), /badDetail/);
  assert.throws(() => normalizeDetail({ ...source, files: [source.files[0], source.files[0]] }, '42'), /badDetail/);
  assert.equal(plainDescription('<script>bad()</script><p>A &lt; B</p><p>C</p>'), 'A < B\nC');
  assert.ok(plainDescription('x'.repeat(100000)).length <= 16000);
});
test('closing detail or opening another mod discards the old response', async t => {
  const { calls, features } = fixture(t);
  const first = features.openDetail('42');
  features.closeDetail();
  const second = features.openDetail('43');
  calls[0].resolve(detail('42')); assert.equal(await first, false);
  assert.equal(features.state.detail.id, '43');
  calls[1].resolve(detail('43')); assert.equal(await second, true);
  assert.equal(features.state.detail.value.id, '43');
});
test('a failed detail request is explicitly retriable without a background loop', async t => {
  const { calls, features } = fixture(t);
  const first = features.openDetail('42');
  calls[0].reject(new Error('offline')); assert.equal(await first, false);
  assert.equal(features.state.detail.status, 'error'); assert.equal(calls.length, 1);
  const retry = features.openDetail('42'); calls[1].resolve(detail('42')); await retry;
  assert.equal(features.state.detail.status, 'ready');
});
test('real AppModel game/profile changes invalidate pending public details', async t => {
  const { calls, features, model } = fixture(t);
  const first = features.openDetail('42');
  model.setShopGame('toby.undertale');
  calls[0].resolve(detail('42')); assert.equal(await first, false);
  assert.equal(features.state.detail.status, 'idle');
  const second = features.openDetail('43');
  model.acceptSnapshot(snapshot(), true, true);
  calls[1].resolve(detail('43')); assert.equal(await second, false);
});
test('reviewing a link has no network side effects until confirmation', async t => {
  const { calls, features, model } = fixture(t);
  assert.equal(features.reviewLink('deltamod-gpuix-preview://browse?game=toby.undertale&q=Snow'), true);
  assert.equal(calls.length, 0); assert.equal(model.state.route, 'home');
  assert.equal(features.applyLink(), true);
  assert.equal(model.state.route, 'shop');
  assert.equal(calls[0].command, 'shop.browse');
  assert.deepEqual(calls[0].args, { query: 'Snow', page: 1, gameId: 'toby.undertale' });
  calls[0].resolve({ items: [], hasMore: false });
  await Promise.resolve();
  assert.equal(features.state.pendingLink, null);
});
test('links to unsupported games or busy state cannot force navigation', t => {
  const { calls, features, model } = fixture(t);
  assert.equal(features.reviewLink('deltamod-gpuix-preview://browse?game=unknown.game'), false);
  assert.equal(calls.length, 0);
  features.reviewLink('deltamod-gpuix-preview://screen?route=themes');
  model.update({ loading: true });
  assert.equal(features.applyLink(), false);
  assert.equal(model.state.route, 'home');
  model.update({ loading: false });
  assert.equal(features.applyLink(), true); assert.equal(model.state.route, 'themes');
});
test('dispose removes model subscriptions and ignores late feature data', async t => {
  const { calls, features, model } = fixture(t);
  let notices = 0; features.subscribe(() => notices++);
  const pending = features.openDetail('42');
  features.dispose(); const before = notices;
  calls[0].resolve(detail('42')); await pending;
  model.setShopGame('toby.undertale');
  assert.equal(notices, before); assert.equal(features.state.detail.value, null);
  assert.equal(model.listeners.size, 0);
  assert.equal(await features.openDetail('43'), false); assert.equal(calls.length, 1);
});
test('new presentation RPC commands stay inside the existing bounded stdio allowlist', async t => {
  const child = new EventEmitter();
  child.stdout = new PassThrough(); child.stderr = new PassThrough(); child.stdin = new PassThrough();
  child.exitCode = null; child.signalCode = null; child.kill = () => { child.signalCode = 'SIGTERM'; };
  const frames = []; child.stdin.on('data', bytes => frames.push(JSON.parse(String(bytes))));
  const bridge = new Bridge(child); t.after(() => bridge.close());
  for (const command of ['ui.preferences.get', 'ui.preferences.set', 'theme.preview', 'shop.detail']) {
    const request = bridge.request(command, {}); const frame = frames.at(-1);
    assert.equal(frame.command, command);
    child.stdout.write(JSON.stringify({ v: 1, id: frame.id, ok: true, result: {} }) + '\n');
    await request;
  }
  await assert.rejects(bridge.request('file.write', { path: '/source' }), /Unsupported/);
  await assert.rejects(bridge.request('http.fetch', { url: 'https://evil.test' }), /Unsupported/);
});
