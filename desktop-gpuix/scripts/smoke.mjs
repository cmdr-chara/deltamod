// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { launch } from '@gpuix/react/automation';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

const output = path.resolve(process.argv[2] || 'smoke-results');
if (fs.existsSync(output)) throw new Error('Choose a new screenshot evidence directory.');
fs.mkdirSync(output, { recursive: true });
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-gpuix-smoke-'));
let app;
try {
  const entry = fileURLToPath(new URL('../dist/main.js', import.meta.url));
  app = await launch({ command: process.execPath, args: [entry, '--state-root', path.join(temporary, 'state'), '--no-focus', '--reduce-motion', '--opaque'] });
  await app.getByTestId('home-ready').waitFor();
  for (const route of ['home', 'library', 'installations', 'shop', 'themes', 'settings']) {
    await app.getByTestId(`nav-${route}`).click();
    await app.screenshot({ path: path.join(output, `${route}.png`) });
  }
  await app.getByTestId('collapse-sidebar').click();
  await app.screenshot({ path: path.join(output, 'compact-sidebar.png') });
  // This smoke only confirms native rendering/navigation, not provider uptime,
  // real video playback, patching, account login or migration parity.
} finally {
  try { if (app) await app.close(); }
  finally { fs.rmSync(temporary, { recursive: true, force: true, maxRetries: 3 }); }
}
