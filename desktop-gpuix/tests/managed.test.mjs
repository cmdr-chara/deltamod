// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { ManagedRuntime, normalizeManagedCatalog, operationId } from '../src/managed.mjs';

const catalog = () => ({
  installedMods: [{
    instanceId: 'sample', modId: 'sample', installationId: 'local-mod-library',
    displayName: 'Sample Mod', version: '1.0', provider: { providerId: 'local' }, files: [{}, {}],
  }],
  verificationResults: [], lifecycleJournals: [], operationRecords: [], errors: [], gameHealthReports: [],
});
function fixture(responses = {}) {
  const calls = [];
  const bridge = { async request(command, args = {}) {
    calls.push({ command, args });
    if (command in responses) {
      const value = responses[command];
      if (value instanceof Error) throw value;
      return typeof value === 'function' ? value(args) : value;
    }
    if (command === 'managed.catalog') return catalog();
    if (command === 'managed.installations') return [];
    if (command === 'managed.game.info') return { id: 'toby.deltarune' };
    if (command === 'managed.credentials.status') return { present: {} };
    if (command === 'managed.mod.states') return { enabled: ['sample'] };
    return true;
  }};
  return { bridge, calls };
}

test('managed catalogue is bounded and preserves lifecycle identity', () => {
  const value = normalizeManagedCatalog(catalog());
  assert.equal(value.mods[0].instanceId, 'sample');
  assert.equal(value.mods[0].installationId, 'local-mod-library');
  assert.equal(value.mods[0].files, 2);
  assert.throws(() => normalizeManagedCatalog({ installedMods: [] }));
  assert.throws(() => normalizeManagedCatalog({ ...catalog(), installedMods: [null] }));
});

test('managed initialization combines catalogue, game, credentials and enabled state', async () => {
  const { bridge, calls } = fixture();
  const runtime = new ManagedRuntime(bridge);
  assert.equal(await runtime.initialize(), true);
  assert.equal(runtime.state.catalog.mods.length, 1);
  assert.deepEqual(runtime.state.enabledIds, ['sample']);
  assert.equal(calls.filter(call => call.command === 'managed.catalog').length, 1);
  runtime.dispose();
});

test('mutations publish only after native acknowledgement and then refresh', async () => {
  let resolve;
  const pending = new Promise(value => { resolve = value; });
  const { bridge, calls } = fixture({ 'managed.mod.toggle': () => pending });
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const action = runtime.toggle('sample', false);
  assert.equal(runtime.state.busy, 'managed.mod.toggle');
  assert.deepEqual(runtime.state.enabledIds, ['sample']);
  resolve(true);
  assert.equal(await action, true);
  assert.equal(runtime.state.busy, '');
  assert.ok(calls.filter(call => call.command === 'managed.catalog').length >= 2);
});

test('failed lifecycle mutations keep the last acknowledged catalogue', async () => {
  const { bridge } = fixture({ 'managed.mod.uninstall': new Error('recovery_required') });
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const before = runtime.state.catalog;
  assert.equal(await runtime.uninstall(before.mods[0]), false);
  assert.equal(runtime.state.catalog, before);
  assert.match(runtime.state.error, /recovery_required/);
});

test('repair, uninstall and restore use unique bounded operation ids', async () => {
  const { bridge, calls } = fixture();
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const mod = runtime.state.catalog.mods[0];
  await runtime.repair(mod); await runtime.uninstall(mod); await runtime.restore('local-mod-library');
  const ids = calls.filter(call => ['managed.mod.repair','managed.mod.uninstall','managed.restore'].includes(call.command))
    .map(call => call.args.operationId);
  assert.equal(new Set(ids).size, 3);
  assert.ok(ids.every(id => /^[a-z]+-[0-9a-f]{32}$/.test(id)));
});

test('archive replacement is always an explicit call-site choice', async () => {
  const { bridge, calls } = fixture();
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  await runtime.importArchive('/tmp/mod.zip', false);
  await runtime.importArchive('/tmp/mod.zip', true);
  const imports = calls.filter(call => call.command === 'managed.importArchive');
  assert.deepEqual(imports.map(call => call.args.replaceExisting), [false, true]);
});

test('patch requests deduplicate and cap selection before crossing the bridge', async () => {
  const { bridge, calls } = fixture();
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  await runtime.patch(['a', 'a', 'b']);
  const call = calls.find(call => call.command === 'managed.patch.run');
  assert.deepEqual(call.args.selected, ['a', 'b']);
});

test('dispose prevents later managed actions', async () => {
  const { bridge, calls } = fixture();
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const count = calls.length;
  runtime.dispose();
  assert.equal(await runtime.launch(), false);
  assert.equal(calls.length, count);
});

test('operation ids are process-local random identifiers, not user paths', () => {
  const first = operationId('repair');
  const second = operationId('repair');
  assert.notEqual(first, second);
  assert.match(first, /^repair-[0-9a-f]{32}$/);
});


test('pinned GPUIX 0.10 UI uses only exported host primitives', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('../src/ui.tsx', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /\b(Button|Dialog|DialogPortal|DialogBackdrop|DialogPopup|DialogTitle|DialogDescription|DialogClose)\b/);
  assert.match(source, /useGpuixRequired/);
  assert.match(source, /role="dialog"/);
  assert.match(source, /focusNextWithin/);
});


test('patch cancellation bypasses the busy mutation gate', async () => {
  let resolvePatch;
  const pending = new Promise(resolve => { resolvePatch = resolve; });
  const { bridge, calls } = fixture({
    'managed.patch.run': () => pending,
    'managed.patch.cancel': true,
  });
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const patch = runtime.patch(['sample']);
  assert.equal(runtime.state.busy, 'managed.patch.run');
  assert.equal(await runtime.cancelPatch(), true);
  assert.ok(calls.some(call => call.command === 'managed.patch.cancel'));
  resolvePatch(true);
  await patch;
});

test('Nexus login has an explicit cancellation path while authorization is pending', async () => {
  let resolveLogin;
  const pending = new Promise(resolve => { resolveLogin = resolve; });
  const { bridge, calls } = fixture({
    'managed.nexus.login': () => pending,
    'managed.nexus.cancel': true,
  });
  const runtime = new ManagedRuntime(bridge);
  await runtime.initialize();
  const login = runtime.loginNexus();
  assert.equal(runtime.state.busy, 'managed.nexus.login');
  assert.equal(await runtime.cancelNexus(), true);
  assert.ok(calls.some(call => call.command === 'managed.nexus.cancel'));
  resolveLogin(true);
  await login;
});
