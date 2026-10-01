// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { resolveStandaloneLocks, resolutionMode } from '../scripts/resolve-locks.mjs';

function fixture(t) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'gpuix-lock-fixture-'));
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }));
  const root = path.join(repo, 'desktop-gpuix');
  fs.mkdirSync(path.join(root, 'native'), { recursive: true });
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'fixture', version: '1.0.0' }));
  fs.writeFileSync(path.join(repo, 'package-lock.json'), 'protected fixture');
  const npm = path.join(repo, 'npm-cli-fixture.js'); fs.writeFileSync(npm, 'fixture');
  return { root, repo, npm };
}
function npmLock(root) {
  // Only a temporary fixture for the resolver contract, never a repository lock.
  fs.writeFileSync(path.join(root, 'package-lock.json'), JSON.stringify({ lockfileVersion: 3, name: 'fixture', version: '1.0.0', packages: { '': {} } }));
}

test('missing Cargo does not prevent the independent npm resolver', t => {
  const { root, npm } = fixture(t), calls = [];
  assert.throws(() => resolveStandaloneLocks(root, { npm, spawnImpl(command, args, options) {
    calls.push(command); assert.equal(options.cwd, root); assert.equal(options.shell, false);
    if (command === 'cargo') return { error: new Error('ENOENT') };
    assert.ok(args.includes('--ignore-scripts')); assert.ok(args.includes('--workspaces=false'));
    npmLock(root); return { status: 0 };
  } }), /Cargo: ENOENT/);
  assert.deepEqual(calls, [process.execPath, 'cargo']);
  assert.ok(fs.existsSync(path.join(root, 'package-lock.json')));
  assert.equal(fs.existsSync(path.join(root, 'native', 'Cargo.lock')), false);
});

test('npm-only does not invoke Cargo or require a fake native lock', t => {
  const { root, npm } = fixture(t);
  assert.deepEqual(resolveStandaloneLocks(root, { mode: 'npm', npm, spawnImpl(command) {
    assert.equal(command, process.execPath); npmLock(root); return { status: 0 };
  } }), ['npm']);
});

test('an npm error does not block an otherwise feasible Cargo resolver', t => {
  const { root, npm } = fixture(t), calls = [];
  assert.throws(() => resolveStandaloneLocks(root, { npm, spawnImpl(command) {
    calls.push(command);
    if (command !== 'cargo') return { status: 1 };
    fs.writeFileSync(path.join(root, 'native', 'Cargo.lock'), 'fixture only'); return { status: 0 };
  } }), /npm:/);
  assert.deepEqual(calls, [process.execPath, 'cargo']);
});

test('protected root lock mutation aborts publication readiness', t => {
  const { root, repo, npm } = fixture(t);
  assert.throws(() => resolveStandaloneLocks(root, { mode: 'npm', npm, spawnImpl() {
    npmLock(root); fs.writeFileSync(path.join(repo, 'package-lock.json'), 'unexpected change'); return { status: 0 };
  } }), /protected Tauri\/root lock changed/);
});

test('successful process exit without a real output lock is not success', t => {
  const { root, npm } = fixture(t);
  assert.throws(() => resolveStandaloneLocks(root, { mode: 'npm', npm, spawnImpl() { return { status: 0 }; } }), /incomplete/);
});

test('resolver modes reject unknown or duplicate arguments', () => {
  assert.equal(resolutionMode([]), 'all'); assert.equal(resolutionMode(['--npm-only']), 'npm');
  assert.equal(resolutionMode(['--cargo-only']), 'cargo');
  for (const args of [['--force'], ['--npm-only', '--cargo-only'], 'npm']) assert.throws(() => resolutionMode(args));
});
