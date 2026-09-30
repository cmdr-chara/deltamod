// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { UpdateRuntime, GPUIX_VERSION, GPUIX_UPDATE_ENDPOINT } from '../src/updater.mjs';

test('GPUIX updater is isolated from the production Tauri feed', () => {
  assert.equal(GPUIX_VERSION, '2.0.18');
  assert.match(GPUIX_UPDATE_ENDPOINT, /latest-gpuix\.json$/);
  assert.doesNotMatch(GPUIX_UPDATE_ENDPOINT, /\/latest\.json$/);
});

test('check publishes an available update only after native verification returns it', async () => {
  let resolve;
  const pending = new Promise(value => { resolve = value; });
  const update = { version: '2.0.19', async downloadAndInstall() {} };
  const runtime = new UpdateRuntime(() => pending);
  const action = runtime.check();
  assert.equal(runtime.state.status, 'checking');
  assert.equal(runtime.state.update, null);
  resolve(update);
  assert.equal(await action, true);
  assert.equal(runtime.state.status, 'available');
  assert.equal(runtime.state.update, update);
});

test('install waits for signature-verified native installer completion', async () => {
  let installed = false;
  const update = { version: '2.0.19', async downloadAndInstall() { installed = true; } };
  const runtime = new UpdateRuntime(async () => update);
  await runtime.check();
  assert.equal(await runtime.install(), true);
  assert.equal(installed, true);
  assert.equal(runtime.state.status, 'installed');
  assert.equal(runtime.state.update, null);
});

test('updater failures remain visible and are never converted into current', async () => {
  const runtime = new UpdateRuntime(async () => { throw new Error('signature mismatch'); });
  assert.equal(await runtime.check(), false);
  assert.equal(runtime.state.status, 'error');
  assert.match(runtime.state.error, /signature mismatch/);
});

test('dispose ignores future update work', async () => {
  let calls = 0;
  const runtime = new UpdateRuntime(async () => { calls++; return null; });
  runtime.dispose();
  assert.equal(await runtime.check(), false);
  assert.equal(calls, 0);
});
