// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setImmediate as nextTurn } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { startSingleInstance } from '../src/single-instance.mjs';
import { parseOptions } from '../src/options.mjs';

function directory(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'gpuix-forward-only-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

test('a forward-only OS helper reaches the existing authenticated listener', async t => {
  const stateRoot = directory(t), received = [];
  const primary = await startSingleInstance({ stateRoot }, [], items => {
    received.push(items); return { accepted: items.length, duplicate: 0 };
  });
  t.after(() => primary.dispose());
  const items = [{ kind: 'preview', value: 'deltamod-gpuix-preview://mod?id=42' }];
  const secondary = await startSingleInstance({ stateRoot, forwardOnly: true }, items, () => assert.fail('must not become primary'));
  assert.equal(secondary.primary, false);
  assert.equal(secondary.accepted, 1);
  assert.deepEqual(received, [[], items]);
});

test('forward-only never binds a listener after primary shutdown', async t => {
  const stateRoot = directory(t);
  const primary = await startSingleInstance({ stateRoot }, [], () => ({ accepted: 0, duplicate: 0 }));
  primary.dispose(); await nextTurn();
  await assert.rejects(startSingleInstance({ stateRoot, forwardOnly: true }, [], () => assert.fail('must not elect a new writer')));
  // The failed forward-only request must not retain the workspace port.
  const replacement = await startSingleInstance({ stateRoot }, [], () => ({ accepted: 0, duplicate: 0 }));
  assert.equal(replacement.primary, true); replacement.dispose();
});

test('forward-only preserves custom managed identity rather than using its own state root', async t => {
  const managedDataRoot = directory(t), received = [];
  const primary = await startSingleInstance({ stateRoot: directory(t), managedDataRoot }, [], items => {
    received.push(items); return { accepted: items.length, duplicate: 0 };
  });
  t.after(() => primary.dispose());
  const result = await startSingleInstance({ stateRoot: directory(t), managedDataRoot, forwardOnly: true }, [], () => assert.fail());
  assert.equal(result.primary, false); assert.equal(received.length, 2);
});

test('CLI marker and forward-only arguments retain strict positional parsing', () => {
  const file = path.resolve('launch.deltamod-open');
  const options = parseOptions(['--forward-only', '--', pathToFileURL(file).href]);
  assert.equal(options.forwardOnly, true); assert.equal(options.launchMarker, file);
  assert.equal(options.archiveFile, ''); assert.equal(options.openLink, '');
  assert.throws(() => parseOptions(['--forward-only', '--forward-only']), /Duplicate/);
  assert.throws(() => parseOptions(['--', '--managed-data-root']), /Unsupported positional/);
  assert.throws(() => parseOptions(['--benchmark-file', path.resolve('bench'), file]), /benchmark/);
});
