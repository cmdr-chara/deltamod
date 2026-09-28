// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { spawnSync } = require('node:child_process');
const source = process.env.DELTAMOD_AUDIT_SOURCE_ROOT || path.resolve(__dirname, '../../..');
const { generate, updaterTarget } = require(path.join(source, 'scripts/generate-tauri-updater-manifest.js'));
const version = '2.0.18';
const tag = `community-v${version}`;
const names = [`Deltamod Community_${version}_x64-setup.exe`, `Deltamod Community_${version}_x64.app.tar.gz`, `Deltamod Community_${version}_aarch64.app.tar.gz`];
function artifacts(t, selected = names) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-updater-audit-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const name of selected) {
    fs.writeFileSync(path.join(root, name), 'fixture artifact, not a package');
    fs.writeFileSync(path.join(root, `${name}.sig`), 'fixture-signature, not cryptographic evidence');
  }
  return root;
}
function symlink(t, target, link, type) {
  try { fs.symlinkSync(target, link, type); return true; }
  catch (error) { if (['EPERM', 'ENOTSUP'].includes(error.code)) { t.skip('symlinks unavailable to this account'); return false; } throw error; }
}
test('stable metadata and rehearsal metadata preserve supported targets', t => {
  const root = artifacts(t);
  const manifest = generate(root, tag);
  assert.equal(manifest.version, version);
  assert.deepEqual(Object.keys(manifest.platforms).sort(), ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64']);
  assert.ok(manifest.platforms['windows-x86_64'].url.endsWith(encodeURIComponent(names[0])));
  assert.equal(generate(root, `updater-rehearsal-v${version}-run-123`, version).version, version);
});
test('missing targets, duplicate targets and unbound tags fail', t => {
  const root = artifacts(t);
  assert.throws(() => generate(root, 'community-v2.0.19', version));
  assert.throws(() => generate(root, `${tag}-beta`, version));
  fs.unlinkSync(path.join(root, `${names[2]}.sig`));
  assert.throws(() => generate(root, tag), /Missing/);
  fs.writeFileSync(path.join(root, `${names[2]}.sig`), 'signature');
  fs.copyFileSync(path.join(root, names[0]), path.join(root, `copy_${version}_x64.exe`));
  fs.copyFileSync(path.join(root, `${names[0]}.sig`), path.join(root, `copy_${version}_x64.exe.sig`));
  assert.throws(() => generate(root, tag), /Duplicate/);
});
for (const impostor of ['12.0.18', '2.0.180', '2.0.18-beta', '2.0.18+build']) {
  test(`rejects an artifact with version ${impostor}`, t => {
    const root = artifacts(t, names.map(name => name.replace(version, impostor)));
    assert.throws(() => generate(root, tag), /bound to release version/);
  });
}
for (const name of ['app_2.0.18_arm64.exe', 'app_2.0.18_ia32.exe', 'app_2.0.18.exe', 'app_2.0.18_x64_arm64.exe', 'app_2.0.18_x64_arm64.app.tar.gz', 'app_2.0.18_notx64.app.tar.gz']) {
  test(`rejects unbound or unsupported architecture: ${name}`, () => {
    assert.throws(() => updaterTarget(name), /architecture/i);
  });
}
test('signature size is rejected before any unbounded read', t => {
  const root = artifacts(t);
  const sig = path.join(root, `${names[0]}.sig`);
  fs.writeFileSync(sig, Buffer.alloc(16385, 65));
  const original = fs.readFileSync;
  let readSignature = false;
  fs.readFileSync = function(file, ...rest) { if (file === sig) readSignature = true; return original.call(this, file, ...rest); };
  try { assert.throws(() => generate(root, tag)); }
  finally { fs.readFileSync = original; }
  assert.equal(readSignature, false, 'must reject before reading the oversized signature');
});
test('signature growth during open stays bounded', t => {
  const root = artifacts(t);
  const sig = path.join(root, `${names[0]}.sig`);
  const original = fs.openSync;
  let grown = false;
  fs.openSync = function(file, flags, ...rest) {
    if (file === sig && typeof flags === 'number' && !grown) {
      grown = true;
      fs.appendFileSync(sig, Buffer.alloc(16385, 65));
    }
    return original.call(this, file, flags, ...rest);
  };
  try { assert.throws(() => generate(root, tag)); }
  finally { fs.openSync = original; }
  assert.equal(grown, true);
});
for (const payload of [Buffer.from([0xff, 0xfe]), Buffer.from('signature\0suffix'), Buffer.from('PRIVATE KEY'), Buffer.from('   ')]) {
  test(`invalid signature bytes fail: ${payload.toString('hex')}`, t => {
    const root = artifacts(t);
    fs.writeFileSync(path.join(root, `${names[0]}.sig`), payload);
    assert.throws(() => generate(root, tag));
  });
}
for (const suffix of ['', '.sig']) {
  test(`rejects a linked ${suffix ? 'signature' : 'artifact'}`, t => {
    const root = artifacts(t);
    const external = artifacts(t, []);
    const link = path.join(root, `${names[0]}${suffix}`);
    const target = path.join(external, 'outside');
    fs.writeFileSync(target, 'external data');
    fs.unlinkSync(link);
    if (!symlink(t, target, link, 'file')) return;
    assert.throws(() => generate(root, tag), /[Ll]ink|ordinary|regular/);
    assert.equal(fs.readFileSync(target, 'utf8'), 'external data');
  });
}
test('rejects linked roots and nested directories', t => {
  const root = artifacts(t);
  const parent = artifacts(t, []);
  const link = path.join(parent, 'linked-root');
  if (!symlink(t, root, link, process.platform === 'win32' ? 'junction' : 'dir')) return;
  assert.throws(() => generate(link, tag), /ordinary/);
  assert.throws(() => generate(parent, tag), /[Ll]ink/);
});
test('bounds directory depth without recursive call-stack growth', t => {
  const root = artifacts(t);
  let nested = root;
  for (let i = 0; i < 18; i++) { nested = path.join(nested, 'd'); fs.mkdirSync(nested); }
  assert.throws(() => generate(root, tag), /depth/);
});
test('bounds total entries', t => {
  const root = artifacts(t);
  for (let i = 0; i < 4100; i++) fs.writeFileSync(path.join(root, `extra-${i}`), '');
  assert.throws(() => generate(root, tag), /entry limit/);
});
test('rejects FIFO artifacts before any blocking open', { skip: process.platform === 'win32' }, t => {
  const root = artifacts(t);
  const fifo = path.join(root, 'unsafe');
  const result = spawnSync('mkfifo', [fifo]);
  if (result.error && result.error.code === 'ENOENT') { t.skip('mkfifo unavailable'); return; }
  assert.equal(result.status, 0);
  assert.throws(() => generate(root, tag), /regular file/);
});
test('deterministic architecture fuzz: 10000 names cannot smuggle another architecture', () => {
  let state = 0x51a7e;
  const next = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state; };
  for (let i = 0; i < 10000; i++) {
    const prefix = `app${next().toString(36)}_${version}_`;
    const suffix = i % 2 ? '.exe' : '.app.tar.gz';
    assert.throws(() => updaterTarget(`${prefix}x64_arm64${suffix}`));
    assert.equal(updaterTarget(`${prefix}x64${suffix}`), suffix === '.exe' ? 'windows-x86_64' : 'darwin-x86_64');
  }
});
