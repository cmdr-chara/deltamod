// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { UpdateRuntime, updateCapability, GPUIX_VERSION, GPUIX_UPDATE_ENDPOINT, GPUIX_UPDATE_PUBLIC_KEY } from '../src/updater.mjs';
const release = Object.freeze({enabled:true,format:'nsis',reason:''});

test('GPUIX updater is isolated from the production Tauri feed', () => {
  assert.equal(GPUIX_VERSION, '2.0.18');
  assert.match(GPUIX_UPDATE_ENDPOINT, /latest-gpuix\.json$/);
  assert.doesNotMatch(GPUIX_UPDATE_ENDPOINT, /\/latest\.json$/);
});

test('check publishes an available update only after the native metadata check returns it', async () => {
  let resolve;
  const pending = new Promise(value => { resolve = value; });
  const update = { version: '2.0.19', format:'nsis', async downloadAndInstall() {} };
  const runtime = new UpdateRuntime(() => pending, release);
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
  const update = { version: '2.0.19', format:'nsis', async downloadAndInstall() { installed = true; } };
  const runtime = new UpdateRuntime(async (_version,options) => {
    assert.equal(options.pubkey,GPUIX_UPDATE_PUBLIC_KEY);
    assert.deepEqual(options.endpoints,[GPUIX_UPDATE_ENDPOINT]);
    return update;
  },release);
  await runtime.check();
  assert.equal(await runtime.install(), true);
  assert.equal(installed, true);
  assert.equal(runtime.state.status, 'installed');
  assert.equal(runtime.state.update, null);
});

test('updater failures remain visible and are never converted into current', async () => {
  const runtime = new UpdateRuntime(async () => { throw new Error('signature mismatch'); },release);
  assert.equal(await runtime.check(), false);
  assert.equal(runtime.state.status, 'error');
  assert.match(runtime.state.error, /signature mismatch/);
});

test('dispose ignores future update work', async () => {
  let calls = 0;
  const runtime = new UpdateRuntime(async () => { calls++; return null; },release);
  runtime.dispose();
  assert.equal(await runtime.check(), false);
  assert.equal(calls, 0);
});

test('unverified previews neither contact a feed nor run an installer', async () => {
  let called=false;
  const runtime=new UpdateRuntime(async()=>{called=true;return null;});
  assert.equal(await runtime.check(),false);
  assert.equal(await runtime.install(),false);
  assert.equal(called,false);
  assert.match(runtime.state.error,/disabled/);
});

test('wrong package formats cannot become installable', async () => {
  const runtime=new UpdateRuntime(async()=>({format:'app',downloadAndInstall(){throw Error('must not execute');}}),release);
  assert.equal(await runtime.check(),false);
  assert.equal(runtime.state.update,null);
  assert.equal(await runtime.install(),false);
});

test('readiness rejects missing/unsigned/wrong-version/wrong-target metadata and Linux', () => {
  const manifest={target:'win32-x64',appVersion:GPUIX_VERSION,releaseChannel:'stable-gpuix',updaterRehearsal:'passed'};
  assert.equal(updateCapability(manifest,'win32','x64').enabled,true);
  for(const value of [null,{}, {...manifest,releaseChannel:'preview'}, {...manifest,updaterRehearsal:'pending'}, {...manifest,appVersion:'0.0.0'}]) {
    assert.equal(updateCapability(value,'win32','x64').enabled,false);
  }
  assert.equal(updateCapability({...manifest,target:'linux-x64'},'linux','x64').enabled,false);
  assert.equal(updateCapability(manifest,'darwin','arm64').enabled,false);
});
