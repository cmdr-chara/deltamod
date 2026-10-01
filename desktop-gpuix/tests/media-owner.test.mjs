// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { PlaybackCoordinator } from '../src/media-owner.mjs';
const owner = () => ({ disposed: false, blocked: false, children: new Set(), async stop() { this.children.clear(); } });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
test('a different theme cannot launch until the old decoder closes', async () => {
  const coordinator = new PlaybackCoordinator();
  const a = owner(), b = owner(), closed = deferred();
  await coordinator.claim(a, async () => { a.children.add('decoder'); return true; });
  a.stop = async () => { await closed.promise; a.children.clear(); };
  let started = false;
  const replacement = coordinator.claim(b, async () => { started = true; return true; });
  await Promise.resolve();
  assert.equal(started, false);
  closed.resolve();
  assert.equal(await replacement, true);
  assert.equal(a.children.size, 0);
});
test('rapid remounts start only the newest requested theme', async () => {
  const coordinator = new PlaybackCoordinator();
  const a = owner(), b = owner(), c = owner();
  const starts = [];
  const first = coordinator.claim(a, async () => { starts.push('a'); return true; });
  const second = coordinator.claim(b, async () => { starts.push('b'); return true; });
  const third = coordinator.claim(c, async () => { starts.push('c'); return true; });
  assert.deepEqual(await Promise.all([first, second, third]), [false, false, true]);
  assert.deepEqual(starts, ['c']);
});
test('an unreaped old decoder blocks all replacement playback', async () => {
  const coordinator = new PlaybackCoordinator();
  const a = owner(), b = owner();
  await coordinator.claim(a, async () => true);
  a.stop = async () => { a.blocked = true; a.children.add('unreaped'); };
  await assert.rejects(coordinator.claim(b, async () => { assert.fail('must not overlap'); }), /could not be stopped/);
  await assert.rejects(coordinator.claim(b, async () => true), /could not be stopped/);
});
test('disposed pending owners do not start a decoder', async () => {
  const coordinator = new PlaybackCoordinator();
  const a = owner();
  const pending = coordinator.claim(a, async () => { assert.fail('disposed'); });
  a.disposed = true;
  assert.equal(await pending, false);
});
test('a rejected start does not permanently poison subsequent playback', async () => {
  const coordinator = new PlaybackCoordinator();
  await assert.rejects(coordinator.claim(owner(), async () => { throw new Error('start failed'); }), /start failed/);
  assert.equal(await coordinator.claim(owner(), async () => true), true);
});
