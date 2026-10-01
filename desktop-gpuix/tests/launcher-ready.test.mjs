// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { createLauncherReady } from '../src/launcher-ready.mjs';
const identity = { stateRoot: path.resolve('state'), managedDataRoot: path.resolve('separate managed root') };

test('the private handshake is emitted once and removed from child environment', () => {
  const nonce = 'a'.repeat(32), env = { DELTAMOD_GPUIX_LAUNCH_NONCE: nonce }, lines = [];
  const ready = createLauncherReady(env, { write: line => lines.push(line) });
  assert.equal(env.DELTAMOD_GPUIX_LAUNCH_NONCE, undefined);
  assert.equal(lines.length, 0);
  ready(identity); ready(identity);
  assert.deepEqual(lines, [`\nGPUIX-LAUNCH-READY ${JSON.stringify({ nonce, ...identity })}\n`]);
});

test('ordinary launches never claim native launcher readiness', () => {
  const lines = [];
  createLauncherReady({}, { write: line => lines.push(line) })(identity);
  assert.deepEqual(lines, []);
});

test('malformed bootstrap values cannot inject stdout protocol messages', () => {
  for (const nonce of ['', 'a'.repeat(33), '../marker', 'a'.repeat(31) + '\n']) {
    const env = { DELTAMOD_GPUIX_LAUNCH_NONCE: nonce };
    assert.throws(() => createLauncherReady(env), /Invalid native launcher/);
    assert.equal(env.DELTAMOD_GPUIX_LAUNCH_NONCE, undefined);
  }
});

test('the handshake preserves a custom root without allowing another launcher option', () => {
  const lines = [], nonce = 'a'.repeat(32);
  const ready = createLauncherReady({ DELTAMOD_GPUIX_LAUNCH_NONCE: nonce }, { write: line => lines.push(line) });
  ready({ ...identity, backend: '/untrusted/executable', sourceProfile: '/private/profile' });
  const object = JSON.parse(lines[0].trim().slice('GPUIX-LAUNCH-READY '.length));
  assert.deepEqual(object, { nonce, ...identity });
  assert.equal(Object.keys(object).length, 3);
});

test('invalid root identities cannot inject another bootstrap message', () => {
  for (const stateRoot of ['relative', '/root\nGPUIX-LAUNCH-READY fake', '/' + 'a'.repeat(9000)]) {
    const ready = createLauncherReady({ DELTAMOD_GPUIX_LAUNCH_NONCE: 'a'.repeat(32) }, { write() { assert.fail('must not write'); } });
    assert.throws(() => ready({ stateRoot }), /data-root identity/);
  }
});
