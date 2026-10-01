// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
// Explicit opt-in decoder smoke. This is not runtime PATH discovery or evidence
// of GPUI painting, an audio device, installed codecs, or A/V synchronization.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { NativeThemePlayer, VIDEO } from '../src/theme-media.mjs';
const ffmpeg = process.env.DELTAMOD_TEST_FFMPEG;
if (!ffmpeg || !path.isAbsolute(ffmpeg)) throw new Error('Provide an absolute DELTAMOD_TEST_FFMPEG for this opt-in smoke.');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'gpuix-native-loop-'));
let player;
try {
  const clip = path.join(root, 'fixture.mp4');
  const generated = spawnSync(ffmpeg, ['-loglevel', 'error', '-f', 'lavfi', '-i', 'color=c=black:s=32x32:r=24',
    '-t', '0.25', '-threads', '1', '-c:v', 'mpeg4', clip], { encoding: 'utf8', shell: false, timeout: 10000, maxBuffer: 65536 });
  if (generated.error || generated.status !== 0) throw generated.error || new Error('Could not generate the disposable media fixture.');
  let frames = 0, timer;
  await new Promise((resolve, reject) => {
    timer = setTimeout(() => reject(new Error('Native loop smoke exceeded its bound.')), 10000);
    player = new NativeThemePlayer({ themeId: 'fixture', video: clip, audio: null, cue: null, soulColor: '#ff0000' },
      { ffmpeg, ffplay: '' }, {
        onFrame(pixels) {
          if (pixels.length !== VIDEO.bytes) { reject(new Error('Invalid native BGRA frame size.')); return; }
          frames++;
          if (frames >= 14) void player.stop();
        },
        onState(state, error) {
          if (state === 'error') reject(new Error(error));
          if (state === 'stopped') frames >= 14 ? resolve() : reject(new Error('The short clip did not repeat.'));
        },
      });
    void player.play({ reducedMotion: false, muted: true, repeat: true }).then(ok => {
      if (!ok) reject(new Error('Native loop playback did not start.'));
    });
  }).finally(() => clearTimeout(timer));
  if (player.children.size !== 0) throw new Error('The native decoder was not reaped.');
  console.log(JSON.stringify({ scope: 'native decoder loop and owned stop only', frames, frameBytes: VIDEO.bytes,
    initialClipSeconds: 0.25, requestedFps: VIDEO.fps, remainingChildren: player.children.size,
    gpuPaintTested: false, audioDeviceTested: false, installedPackageTested: false }, null, 2));
} finally {
  if (player) { await player.stop(); player.dispose(); }
  fs.rmSync(root, { recursive: true, force: true });
}
