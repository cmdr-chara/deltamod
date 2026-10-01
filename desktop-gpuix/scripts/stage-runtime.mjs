// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { requireLocks } from './locks.mjs';
import { GPUIX_VERSION, targetFor, regularFile, verifyBinary, fileDigest, readJsonResource } from '../src/runtime-layout.mjs';
import { mediaTools } from '../src/theme-media.mjs';
const root=path.resolve(fileURLToPath(new URL('..',import.meta.url)));
const pkg=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
const target=targetFor();
const lock=requireLocks(root);
const suffix=process.platform==='win32'?'.exe':'';
const host=regularFile(root,`dist/deltamod-gpuix-host${suffix}`);
verifyBinary(host,target.id);
verifyBinary(regularFile(root,`dist/deltamod-gpuix${suffix}`),target.id);
const launcher=process.platform==='darwin'?regularFile(root,'dist/deltamod-gpuix-launcher'):null;
if(launcher)verifyBinary(launcher,target.id);
const addons=[];
let visited=0;
function walk(directory,depth=0) {
  if(depth>5) throw new Error('Unexpected native package nesting.');
  for(const entry of fs.readdirSync(directory,{withFileTypes:true})) {
    if(++visited>4000 || entry.isSymbolicLink()) throw new Error('Native packages must be bounded, resolved files, not links.');
    const file=path.join(directory,entry.name);
    if(entry.isDirectory()) walk(file,depth+1);
    else if(entry.isFile()&&entry.name===target.addon) addons.push(file);
  }
}
walk(path.join(root,'node_modules','@gpuix'));
if(addons.length!==1) throw new Error(`Expected exactly one installed ${target.addon}, found ${addons.length}.`);
for(const dependency of ['@gpuix/native','@gpuix/react']) {
  const installed=readJsonResource(root,`node_modules/${dependency}/package.json`);
  if(installed.version!==GPUIX_VERSION) throw new Error('GPUIX native/react versions must match the pinned release.');
}
verifyBinary(addons[0],target.id);
const output=path.join(root,'dist','native');
fs.rmSync(output,{recursive:true,force:true});fs.mkdirSync(output,{recursive:true});
fs.copyFileSync(addons[0],path.join(output,target.addon));
const notices=[];
for(const [location,record] of Object.entries(lock.packages)) {
  if(!location || record.dev || !fs.existsSync(path.join(root,location))) continue;
  // Only npm's installed production packages. Never use an arbitrary source
  // path supplied through a theme or provider response.
  if(!/^node_modules\/(?:@[^/]+\/)?[^/]+(?:\/node_modules\/(?:@[^/]+\/)?[^/]+)*$/.test(location)) throw new Error('Invalid installed package path.');
  const directory=path.join(root,location);
  const manifest=readJsonResource(root,`${location}/package.json`);
  if(manifest.version!==record.version) throw new Error(`Installed dependency differs from the npm lock: ${location}`);
  const licenseFiles=fs.readdirSync(directory).filter(name=>/^(licen[sc]e|copying|notice)(\.|$)/i.test(name));
  const slug=location.replaceAll('/','_');
  const destination=path.join(output,'licenses',slug);fs.mkdirSync(destination,{recursive:true});
  fs.copyFileSync(regularFile(directory,'package.json'),path.join(destination,'package.json'));
  for(const file of licenseFiles) fs.copyFileSync(regularFile(directory,file,8*1024*1024),path.join(destination,file));
  if(location.startsWith('node_modules/@gpuix/')&&!licenseFiles.length) throw new Error(`The installed GPUIX package is missing its redistribution license: ${location}`);
  notices.push({name:manifest.name,version:manifest.version,license:manifest.license??null,source:record.resolved,integrity:record.integrity});
}
fs.writeFileSync(path.join(output,'dependency-provenance.json'),JSON.stringify(notices,null,2)+'\n');
const codecRoot=process.env.DELTAMOD_GPUIX_MEDIA_DIR;
if(codecRoot) {
  const source=path.resolve(codecRoot);
  mediaTools(source); // Target, size, provenance and digests before copying.
  const manifest=readJsonResource(source,'native/media.json');
  for(const record of [manifest.ffmpeg,manifest.ffplay,{path:manifest.license}]) {
    if(!record.path.startsWith('native/media/')) throw new Error('Reviewed media artifacts must live below native/media/.');
  }
  let mediaFiles=0;let mediaBytes=0;
  function copyTree(from,to,depth=0) {
    if(depth>8) throw new Error('Media input tree is too deep.');
    fs.mkdirSync(to,{recursive:true});
    for(const entry of fs.readdirSync(from,{withFileTypes:true})) {
      const relative=path.relative(source,path.join(from,entry.name)).split(path.sep).join('/');
      if(entry.isSymbolicLink() || (!entry.isDirectory()&&!entry.isFile())) throw new Error('Media inputs contain links or special files.');
      if(entry.isDirectory()) copyTree(path.join(from,entry.name),path.join(to,entry.name),depth+1);
      else {
        const file=regularFile(source,relative);mediaBytes+=fs.statSync(file).size;
        if(++mediaFiles>2000 || mediaBytes>1024*1024*1024) throw new Error('Media input tree exceeds its staging budget.');
        fs.copyFileSync(file,path.join(to,entry.name));
      }
    }
  }
  copyTree(path.join(source,'native','media'),path.join(output,'media'));
  fs.copyFileSync(regularFile(source,'native/media.json'),path.join(output,'media.json'));
  mediaTools(path.join(root,'dist')); // Verify copied bytes, not just input paths.
}
fs.writeFileSync(path.join(output,'runtime.json'),JSON.stringify({schemaVersion:1,gpuixVersion:GPUIX_VERSION,
  appVersion:pkg.version,target:target.id,releaseChannel:'preview',updaterRehearsal:'not-performed',
  locks:{npm:fileDigest(path.join(root,'package-lock.json')),cargo:fileDigest(path.join(root,'native','Cargo.lock'))},
  launcherSha256:launcher?fileDigest(launcher):null,
  frontendSha256:fileDigest(path.join(root,`dist/deltamod-gpuix${suffix}`)),hostSha256:fileDigest(host),addon:{path:`native/${target.addon}`,sha256:fileDigest(path.join(output,target.addon))}},null,2)+'\n');
console.log(`Staged ${target.id} native runtime. Publisher signing, installed smoke tests and updater rehearsal remain separate gates.`);
