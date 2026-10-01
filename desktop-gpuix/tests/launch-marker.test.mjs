// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { LAUNCH_MARKER, launchMarkerPath, readLaunchMarker, acknowledgeLaunchMarker, abandonLaunchMarker } from '../src/launch-marker.mjs';
function fixture(t) {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'gpuix-marker-'));
  const root = path.join(temporary, 'Deltamod Community CLI');
  fs.mkdirSync(root, { mode: 0o700 });
  const file = path.join(root, 'launch.deltamod-open');
  fs.writeFileSync(file, LAUNCH_MARKER, { mode: 0o600 });
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  return { temporary, root, file };
}
test('trusted CLI marker is removed only after delivery acknowledgement', t => {
  const { temporary, file } = fixture(t);
  const ticket = readLaunchMarker(file, temporary);
  assert.equal(fs.existsSync(file), true);
  assert.equal(acknowledgeLaunchMarker(ticket), true);
  assert.equal(fs.existsSync(file), false);
  assert.equal(acknowledgeLaunchMarker(ticket), false);
});
test('failed or uncertain delivery leaves the marker for its caller', t => {
  const { temporary, file } = fixture(t);
  const ticket = readLaunchMarker(file, temporary);
  abandonLaunchMarker(ticket);
  assert.equal(acknowledgeLaunchMarker(ticket), false);
  assert.equal(fs.readFileSync(file, 'utf8'), LAUNCH_MARKER);
});
test('a forged ticket cannot delete a file', t => {
  const { file } = fixture(t);
  assert.equal(acknowledgeLaunchMarker({ path: file }), false);
  assert.equal(fs.existsSync(file), true);
});
test('rejects marker contents other than the exact CLI protocol', t => {
  const { temporary, file } = fixture(t);
  for (const text of ['', LAUNCH_MARKER.trim(), LAUNCH_MARKER + 'run=anything', 'x'.repeat(LAUNCH_MARKER.length)]) {
    fs.writeFileSync(file, text);
    assert.throws(() => readLaunchMarker(file, temporary));
    assert.equal(fs.existsSync(file), true);
  }
});
test('matching content outside the trusted CLI directory is rejected', t => {
  const { temporary } = fixture(t);
  const outside = path.join(temporary, 'launch.deltamod-open');
  fs.writeFileSync(outside, LAUNCH_MARKER);
  assert.throws(() => readLaunchMarker(outside, temporary));
});
test('an in-place modification after review is not deleted', t => {
  const { temporary, file } = fixture(t);
  const ticket = readLaunchMarker(file, temporary);
  fs.writeFileSync(file, 'x'.repeat(LAUNCH_MARKER.length));
  assert.throws(() => acknowledgeLaunchMarker(ticket));
  assert.equal(fs.existsSync(file), true);
});
test('replacement after review is not deleted even with matching content', t => {
  const { temporary, file } = fixture(t);
  const ticket = readLaunchMarker(file, temporary);
  fs.renameSync(file, file + '.old');
  fs.writeFileSync(file, LAUNCH_MARKER, { mode: 0o600 });
  assert.throws(() => acknowledgeLaunchMarker(ticket));
  assert.equal(fs.existsSync(file), true);
});
test('symlinked files and CLI directories are rejected', { skip: process.platform === 'win32' }, t => {
  const { temporary, root, file } = fixture(t);
  const link = path.join(root, 'link.deltamod-open');
  fs.symlinkSync(file, link);
  assert.throws(() => readLaunchMarker(link, temporary));
  fs.renameSync(root, root + '.old');
  fs.symlinkSync(root + '.old', root);
  assert.throws(() => readLaunchMarker(file, temporary));
});
test('local file URLs retain local-only and extension rules', t => {
  const { file } = fixture(t);
  assert.equal(launchMarkerPath(pathToFileURL(file).href), file);
  for (const raw of ['relative.deltamod-open', '/tmp/file.zip', 'file://remote/tmp/a.deltamod-open', 'file:///tmp/a.deltamod-open?x=1', '/tmp/a\n.deltamod-open']) {
    assert.throws(() => launchMarkerPath(raw));
  }
});
test('Windows marker paths reject shares, devices and alternate streams', () => {
  assert.equal(launchMarkerPath('C:\\Temp\\open.deltamod-open', 'win32'), 'C:\\Temp\\open.deltamod-open');
  for (const raw of ['\\\\server\\share\\a.deltamod-open', 'C:a.deltamod-open', 'C:\\Temp\\a:payload.deltamod-open', '\\\\?\\C:\\a.deltamod-open']) {
    assert.throws(() => launchMarkerPath(raw, 'win32'));
  }
});
