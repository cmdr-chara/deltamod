// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { setImmediate as nextTurn } from 'node:timers/promises';
import { NativeThemePlayer, playbackPlan } from '../src/theme-media.mjs';
import { PlaybackCoordinator } from '../src/media-owner.mjs';
const theme = { themeId: 'fixture', video: '/bundled/video.mp4', audio: '/bundled/audio.mp3', cue: 5.6, soulColor: '#ff0000' };

function spawner() {
  const children = [];
  const spawn = (_executable, args) => {
    const child = new EventEmitter(); child.pid = 123;
    child.stdout = new PassThrough(); child.stderr = new PassThrough(); child.args = args;
    child.kill = () => { queueMicrotask(() => child.emit('close', 0)); return true; };
    children.push(child); return child;
  };
  return { spawn, children };
}

test('looping uses fixed native decoder options and remains explicitly opt-in', () => {
  const once = playbackPlan(theme, { reducedMotion: false, muted: false });
  assert.equal(once.video.includes('-stream_loop'), false); assert.equal(once.audio.includes('-loop'), false);
  const loop = playbackPlan(theme, { reducedMotion: false, muted: false, repeat: true, fromCue: true });
  assert.deepEqual(loop.video.slice(loop.video.indexOf('-stream_loop'), loop.video.indexOf('-stream_loop') + 2), ['-stream_loop', '-1']);
  assert.deepEqual(loop.audio.slice(loop.audio.indexOf('-loop'), loop.audio.indexOf('-loop') + 2), ['-loop', '0']);
  assert.equal(loop.offset, 5.6);
  assert.equal(loop.video.includes('3600'), true); assert.equal(loop.audio.includes('3600'), true);
});

test('zero volume does not spawn a silent audio process', () => {
  const plan = playbackPlan(theme, { reducedMotion: false, muted: false, volume: 0 });
  assert.equal(plan.audio, null); assert.ok(plan.video);
  assert.throws(() => playbackPlan(theme, { reducedMotion: false, repeat: 'yes' }), /Invalid/);
});

test('actual player replacement waits for the old instance to close', async t => {
  const fake = spawner(), coordinator = new PlaybackCoordinator();
  const callbacks = { spawnImpl: fake.spawn, coordinator };
  const old = new NativeThemePlayer(theme, { ffmpeg: '/codec', ffplay: '/audio' }, callbacks);
  const next = new NativeThemePlayer(theme, { ffmpeg: '/codec', ffplay: '/audio' }, callbacks);
  t.after(() => { old.dispose(); next.dispose(); });
  assert.equal(await old.play({ reducedMotion: false }), true);
  const child = fake.children[0]; child.kill = () => true; // Signal alone is not exit.
  const starting = next.play({ reducedMotion: false }); await nextTurn();
  assert.equal(fake.children.length, 1);
  child.emit('close', 0);
  assert.equal(await starting, true); assert.equal(fake.children.length, 2);
  await next.stop(); assert.equal(next.children.size, 0);
});

test('unmounting a queued replacement never starts a decoder', async t => {
  const fake = spawner(), coordinator = new PlaybackCoordinator();
  const old = new NativeThemePlayer(theme, { ffmpeg: '/codec', ffplay: '/audio' }, { spawnImpl: fake.spawn, coordinator });
  const next = new NativeThemePlayer(theme, { ffmpeg: '/codec', ffplay: '/audio' }, { spawnImpl: fake.spawn, coordinator });
  t.after(() => { old.dispose(); next.dispose(); });
  await old.play({ reducedMotion: false }); const child = fake.children[0]; child.kill = () => true;
  const starting = next.play({ reducedMotion: false }); await nextTurn(); next.dispose(); child.emit('close', 0);
  assert.equal(await starting, false); assert.equal(fake.children.length, 1);
});
