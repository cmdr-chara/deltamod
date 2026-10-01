// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { targetFor, verifyBinary } from '../src/runtime-layout.mjs';
const root = path.resolve(import.meta.dirname, '..');
if (process.platform === 'darwin') {
  const target = targetFor();
  const output = path.join(root, 'dist', 'deltamod-gpuix-launcher');
  fs.mkdirSync(path.dirname(output), { recursive: true });
  const result = spawnSync('xcrun', ['swiftc', '-swift-version', '5', '-O',
    '-target', 'arm64-apple-macos13.0', '-framework', 'AppKit',
    path.join(root, 'macos', 'LaunchPolicy.swift'), path.join(root, 'macos', 'Launcher.swift'), '-o', output],
    { cwd: root, stdio: 'inherit', shell: false, timeout: 120000 });
  if (result.error || result.status !== 0) {
    fs.rmSync(output, { force: true });
    throw result.error || new Error('Native macOS launcher compilation failed.');
  }
  verifyBinary(output, target.id);
}
