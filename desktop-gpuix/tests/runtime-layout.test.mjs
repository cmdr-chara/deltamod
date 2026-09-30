// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { binaryTarget, targetFor, regularFile, fileDigest, checkedArtifact, prepareNativeRuntime } from '../src/runtime-layout.mjs';
function header(target) {
  const buffer = Buffer.alloc(256);
  if (target === 'linux-x64') { buffer.set([127,69,76,70,2,1]); buffer.writeUInt16LE(62,18); }
  if (target === 'darwin-arm64') { buffer.writeUInt32LE(0xfeedfacf); buffer.writeUInt32LE(0x0100000c,4); }
  if (target === 'win32-x64') { buffer.write('MZ');buffer.writeUInt32LE(128,60);buffer.write('PE\0\0',128,'binary');buffer.writeUInt16LE(0x8664,132); }
  return buffer;
}
test('native package target matrix does not invent Intel macOS or ARM Linux artifacts', () => {
  for (const [platform,arch] of [['win32','x64'],['linux','x64'],['darwin','arm64']]) assert.equal(targetFor(platform,arch).id,`${platform}-${arch}`);
  for (const [platform,arch] of [['darwin','x64'],['linux','arm64'],['win32','arm64']]) assert.throws(()=>targetFor(platform,arch));
  assert.deepEqual(targetFor('linux','x64').formats,['deb','appimage']);
});
test('binary target reads machine headers rather than trusting filenames', () => {
  for (const target of ['linux-x64','darwin-arm64','win32-x64']) assert.equal(binaryTarget(header(target)),target);
  const wrong=header('linux-x64');wrong.writeUInt16LE(183,18);assert.throws(()=>binaryTarget(wrong));
  const broken=header('win32-x64');broken.writeUInt32LE(999999,60);assert.throws(()=>binaryTarget(broken));
  assert.throws(()=>binaryTarget(Buffer.alloc(63)));
  assert.throws(()=>binaryTarget(Buffer.alloc(128)));
});
test('resource resolution rejects traversal, links, empty and oversize files', t => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpuix-layout-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  fs.mkdirSync(path.join(root,'native'));fs.writeFileSync(path.join(root,'native','real'), 'contents');
  assert.equal(regularFile(root,'native/real'),path.join(root,'native','real'));
  for(const name of ['../real','/real','native//real','native/./real','native/../real','native/real:ads','native\\real','native/real\0']) assert.throws(()=>regularFile(root,name));
  assert.throws(()=>regularFile(root,'native/real',2));
  fs.writeFileSync(path.join(root,'empty'),'');assert.throws(()=>regularFile(root,'empty'));
  if(process.platform!=='win32') { fs.symlinkSync(path.join(root,'native'),path.join(root,'alias'));assert.throws(()=>regularFile(root,'alias/real')); }
});
test('artifact verification checks both machine and digest', t => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpuix-artifact-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const file=path.join(root,'test.node');fs.writeFileSync(file,header('linux-x64'));
  const record={path:'test.node',sha256:fileDigest(file)};
  assert.equal(checkedArtifact(root,record,'linux-x64'),file);
  assert.throws(()=>checkedArtifact(root,record,'win32-x64'));
  assert.throws(()=>checkedArtifact(root,{...record,sha256:'0'.repeat(64)},'linux-x64'));
});
test('compiled launch fails closed without a native manifest, development is unchanged', t => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpuix-launch-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const options={resourcesRoot:root,executable:path.join(root,'host')};
  assert.equal(prepareNativeRuntime(options,{},'/usr/bin/node'),null);
  assert.throws(()=>prepareNativeRuntime(options,{},'/opt/deltamod-gpuix'));
});
