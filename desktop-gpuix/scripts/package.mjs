// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { requireLocks } from './locks.mjs';
import { fileDigest, prepareNativeRuntime, regularFile, targetFor, verifyBinary } from '../src/runtime-layout.mjs';
const root=path.resolve(import.meta.dirname,'..');
const repo=path.resolve(root,'..');
const pkg=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
const product=JSON.parse(fs.readFileSync(path.join(repo,'package.json'),'utf8'));
if(pkg.version!==product.version) throw new Error('GPUIX and product versions must match before packaging.');
// No environment switch may retire Tauri or take over its associations while
// the documented production, signing and platform gates remain incomplete.
if(process.env.DELTAMOD_GPUIX_CUTOVER==='1') throw new Error('GPUIX production cutover is blocked. See MIGRATION-STATUS.md.');
requireLocks(root);
const target=targetFor();
const exe=process.platform==='win32'?'.exe':'';
const frontend=regularFile(root,'dist/deltamod-gpuix'+exe);
const host=regularFile(root,'dist/deltamod-gpuix-host'+exe);
verifyBinary(frontend,target.id);
const manifest=prepareNativeRuntime({resourcesRoot:path.join(root,'dist'),executable:host},{},frontend);
if(manifest.frontendSha256!==fileDigest(frontend)) throw new Error('Frontend changed after native runtime staging.');
if(manifest.appVersion!==pkg.version || manifest.releaseChannel!=='preview' || manifest.updaterRehearsal!=='not-performed') {
  throw new Error('This packaging path accepts preview manifests only.');
}
if(manifest.locks?.npm!==fileDigest(path.join(root,'package-lock.json'))
  || manifest.locks?.cargo!==fileDigest(path.join(root,'native','Cargo.lock'))) throw new Error('Runtime staging used different dependency locks.');
const format=process.env.DELTAMOD_GPUIX_FORMAT || target.formats[0];
if(!target.formats.includes(format)) throw new Error('Unsupported GPUIX package format for this target.');
let icon;
if(process.platform==='darwin'){
  const iconset=path.join(root,'dist','Deltamod.iconset');
  fs.rmSync(iconset,{recursive:true,force:true});fs.mkdirSync(iconset,{recursive:true});
  const source=path.join(repo,'src-tauri','icons','icon.png');
  for(const [size,name] of [[16,'icon_16x16.png'],[32,'icon_16x16@2x.png'],[32,'icon_32x32.png'],[64,'icon_32x32@2x.png'],[128,'icon_128x128.png'],[256,'icon_128x128@2x.png'],[256,'icon_256x256.png'],[512,'icon_256x256@2x.png'],[512,'icon_512x512.png'],[1024,'icon_512x512@2x.png']]){
    const resized=spawnSync('sips',['-z',String(size),String(size),source,'--out',path.join(iconset,name)],{stdio:'inherit',shell:false});
    if(resized.status!==0) throw resized.error||new Error('Could not generate macOS icon.');
  }
  const iconPath=path.join(root,'dist','Deltamod.icns');
  const built=spawnSync('iconutil',['-c','icns',iconset,'-o',iconPath],{stdio:'inherit',shell:false});
  if(built.status!==0) throw built.error||new Error('Could not build macOS icns.');
  icon='dist/Deltamod.icns';
}else icon=process.platform==='win32'?'../src-tauri/icons/icon.ico':'../src-tauri/icons/icon.png';
const config={
  productName:'Deltamod Community GPUIX',version:pkg.version,
  identifier:'io.github.cmdr-chara.deltamod-community-gpuix',
  licenseFile:'../LICENSE.txt',description:'Native GPUIX desktop preview for Deltamod Community',
  publisher:'Deltamod Community contributors',
  fileAssociations:[], // Production .modarchive ownership remains with Tauri.
  binariesDir:'dist',outDir:'bundle',
  binaries:[{path:'deltamod-gpuix',main:true},{path:'deltamod-gpuix-host',main:false}],
  formats:[format],icons:[icon],
  // Do not advertise Launch Services delivery before native open-URL callbacks
  // exist. macOS CLI handoffs remain available for development verification.
  deepLinkProtocols:process.platform==='darwin'?[]:[{schemes:['deltamod-gpuix-preview']}],
  resources:[
    {src:'dist/native',target:'deltamod/native'},
    {src:'../games',target:'deltamod/games'},
    {src:'../web/themes',target:'deltamod/themes'},
    {src:'../web/themes',target:'deltamod/web/themes'},
    {src:'../src-tauri/resources/third-party',target:'deltamod/third-party'},
    {src:'../src-tauri/resources/NOTICE.md',target:'deltamod/NOTICE.md'},
    {src:'../src-tauri/resources/THIRD_PARTY_NOTICES.md',target:'deltamod/THIRD_PARTY_NOTICES.md'},
    {src:'../LICENSE.txt',target:'deltamod/LICENSE.txt'}
  ],
  nsis:{installMode:'currentUser'}
};
if(process.platform==='win32')config.resources.push({src:'../tools/cmodeutil.exe',target:'deltamod/tools/cmodeutil.exe'});
const generated=path.join(root,'dist','packager.generated.json');
fs.writeFileSync(generated,JSON.stringify(config,null,2)+'\n');
const run=spawnSync('cargo',['packager','--release','--config',generated],{cwd:root,stdio:'inherit',shell:false});
if(run.error)throw run.error;
if(run.status!==0)process.exit(run.status??1);
