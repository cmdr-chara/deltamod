// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { requireLocks } from './locks.mjs';
import { regularFile } from '../src/runtime-layout.mjs';

export function resolutionMode(argv) {
  if (!Array.isArray(argv) || argv.length > 1 || (argv.length && !['--npm-only', '--cargo-only'].includes(argv[0]))) {
    throw new Error('Expected no arguments, --npm-only or --cargo-only.');
  }
  return argv.length ? argv[0].slice(2, -5) : 'all';
}

/** The two independent workspaces must not block each other's resolver. Only
 * real package-manager output is accepted. This never fabricates/copies locks. */
export function resolveStandaloneLocks(root, { mode = 'all', npm = process.env.npm_execpath, spawnImpl = spawnSync } = {}) {
  if (!['npm', 'cargo', 'all'].includes(mode) || !path.isAbsolute(root)) throw new Error('Invalid standalone lock resolution request.');
  const repo = path.dirname(root);
  const protectedLocks = ['package-lock.json', 'src-tauri/Cargo.lock', 'native/Cargo.lock'].map(file => path.join(repo, file));
  const digest = file => fs.existsSync(file) ? createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null;
  const before = protectedLocks.map(digest);
  const completed = [], errors = [];
  const run = (command, args) => {
    const result = spawnImpl(command, args, { cwd: root, stdio: 'inherit', shell: false, timeout: 120000 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error('The package manager failed. Its lock is not ready for publication.');
  };
  try {
    if (mode !== 'cargo') {
      try {
        if (!npm || !fs.existsSync(npm)) throw new Error('Run this command through npm run lock:resolve or npm run lock:npm.');
        run(process.execPath, [npm, 'install', '--package-lock-only', '--ignore-scripts', '--no-audit', '--no-fund', '--workspaces=false', '--fetch-retries=0', '--fetch-timeout=10000']);
        const generated = JSON.parse(fs.readFileSync(regularFile(root, 'package-lock.json', 16 * 1024 * 1024), 'utf8'));
        const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
        if (generated.lockfileVersion !== 3 || generated.name !== pkg.name || generated.version !== pkg.version || !generated.packages?.['']) {
          throw new Error('npm did not produce the standalone GPUIX lock.');
        }
        completed.push('npm');
      } catch (error) { errors.push(new Error('npm: ' + error.message)); }
    }
    if (mode !== 'npm') {
      try {
        run('cargo', ['generate-lockfile', '--manifest-path', path.join(root, 'native', 'Cargo.toml')]);
        regularFile(root, 'native/Cargo.lock', 16 * 1024 * 1024);
        completed.push('Cargo');
      } catch (error) { errors.push(new Error('Cargo: ' + error.message)); }
    }
    if (mode === 'all' && errors.length === 0) requireLocks(root);
  } finally {
    if (protectedLocks.some((file, index) => digest(file) !== before[index])) {
      throw new Error('A protected Tauri/root lock changed. Do not publish these inputs.');
    }
  }
  if (errors.length) throw new AggregateError(errors, 'Standalone lock resolution is incomplete:\n' + errors.map(error => error.message).join('\n'));
  return completed;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const root = fileURLToPath(new URL('..', import.meta.url));
    const completed = resolveStandaloneLocks(root, { mode: resolutionMode(process.argv.slice(2)) });
    console.log(`Resolved standalone ${completed.join(' and ')} lock. Review before committing. Full packaging still requires both locks and native validation.`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
