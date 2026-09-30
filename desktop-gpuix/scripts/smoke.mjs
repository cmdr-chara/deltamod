// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { launch } from '@gpuix/react/automation';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const output = path.resolve(process.argv[2] || 'smoke-results');
if (fs.existsSync(output)) throw new Error('Choose a new screenshot evidence directory.');
fs.mkdirSync(output, { recursive: true });
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-gpuix-smoke-'));
const source = path.join(temporary, 'source');
function fixtureFile(relative, value) {
  const file = path.join(source, relative);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(value) + '\n');
}
function sourceDigest() {
  const hash = createHash('sha256');
  function visit(directory, prefix = '') {
    for (const name of fs.readdirSync(directory).sort()) {
      const file = path.join(directory, name);
      const relative = `${prefix}${name}`;
      const stat = fs.lstatSync(file);
      if (stat.isSymbolicLink()) throw new Error('Smoke fixture contains an unexpected link.');
      hash.update(relative + '\0');
      if (stat.isDirectory()) { hash.update('directory\0'); visit(file, `${relative}/`); }
      else if (stat.isFile()) { hash.update(String(stat.size) + '\0'); hash.update(fs.readFileSync(file)); }
      else throw new Error('Unexpected fixture entry.');
    }
  }
  visit(source);
  return hash.digest('hex');
}
let app;
try {
  fixtureFile('profiles/installations.json', {
    current_index: 0, installations: [{ index: 0, name: 'Deltarune fixture' }, { index: 1, name: 'Undertale fixture' }],
  });
  fixtureFile('deltamod_system-0/store.json', { gamePid: 'toby.deltarune', gamePath: '' });
  fixtureFile('deltamod_system-1/store.json', { gamePid: 'toby.undertale', gamePath: '' });
  fixtureFile('mods/fixture-mod/manifest.json', { uid: 'fixture-mod', name: 'Enabled fixture mod' });
  fixtureFile('runtime/mods-state.json', { enabled: ['fixture-mod'] });
  fixtureFile('packets/legacy-fixture/__deltaID.json', { uniqueId: 'legacy-fixture', name: 'Unknown-state fixture mod' });
  const before = sourceDigest();
  const entry = fileURLToPath(new URL('../dist/main.js', import.meta.url));
  app = await launch({ command: process.execPath, args: [entry, '--state-root', path.join(temporary, 'state'),
    '--source-profile', source, '--no-focus', '--reduce-motion', '--opaque'] });
  await app.getByTestId('home-ready').waitFor();
  // Keep provider networking until last: native safety/navigation checks must
  // not depend on GameBanana availability or wait behind a provider request.
  for (const route of ['home', 'library', 'installations', 'themes', 'settings']) {
    await app.getByTestId(`nav-${route}`).click();
    await app.screenshot({ path: path.join(output, `${route}.png`) });
  }
  await app.getByTestId('nav-installations').click();
  await app.getByTestId('select-installation-1').click();
  await app.getByTestId('preview-selected-1').waitFor();
  await app.screenshot({ path: path.join(output, 'preview-selection.png') });
  await app.getByTestId('nav-library').click();
  await app.getByTestId('library-status-enabled').click();
  await app.getByTestId('library-mod-fixture-mod').waitFor();
  await app.screenshot({ path: path.join(output, 'enabled-library.png') });
  await app.getByTestId('library-status-unknown').click();
  await app.getByTestId('library-mod-legacy-fixture').waitFor();
  await app.screenshot({ path: path.join(output, 'unknown-state-library.png') });
  await app.getByTestId('nav-settings').click();
  await app.getByTestId('detach-profile').click();
  await app.getByTestId('confirm-detach-profile').click();
  await app.getByTestId('nav-library').click();
  await app.getByTestId('profile-detached').waitFor();
  await app.screenshot({ path: path.join(output, 'disconnected-profile.png') });
  await app.getByTestId('collapse-sidebar').click();
  await app.screenshot({ path: path.join(output, 'compact-sidebar.png') });
  const after = sourceDigest();
  if (after !== before) throw new Error('Read-only preview changed the source fixture.');
  fs.writeFileSync(path.join(output, 'profile-safety.json'), JSON.stringify({
    schemaVersion: 1, sourceUnchanged: true, beforeSha256: before, afterSha256: after,
    checks: ['session-only-installation-selection', 'enabled-and-unknown-library-filters', 'profile-disconnect'],
  }, null, 2) + '\n', { flag: 'wx' });
  await app.getByTestId('nav-shop').click();
  await app.screenshot({ path: path.join(output, 'shop.png') });
  // Actual GPUI screenshots and fixture safety, not provider uptime, real video,
  // patching, account login, trusted packaging or full migration parity.
} finally {
  try { if (app) await app.close(); }
  finally { fs.rmSync(temporary, { recursive: true, force: true, maxRetries: 3 }); }
}
