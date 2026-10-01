// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { inspectBackendGraph } from './backend-graph.mjs';
const root = path.resolve(import.meta.dirname, '..');
const args = ['metadata', '--format-version', '1', '--locked', '--manifest-path', path.join(root, 'native/Cargo.toml')];
const target = process.argv[2];
if (process.argv.length > 3 || (target && !/^[a-z0-9_]+(?:-[a-z0-9_]+){2,4}$/.test(target))) throw new Error('Expected one optional Cargo target triple.');
if (target) args.push('--filter-platform', target);
const run = spawnSync('cargo', args, { cwd: root, encoding: 'utf8', shell: false, timeout: 60000, maxBuffer: 16 * 1024 * 1024 });
if (run.error) throw run.error;
if (run.status !== 0) throw new Error('Locked Cargo metadata failed. Resolve and review the standalone lock before this check.\n' + run.stderr.slice(-4096));
console.log(JSON.stringify(inspectBackendGraph(JSON.parse(run.stdout)), null, 2));
