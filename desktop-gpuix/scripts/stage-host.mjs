// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
const root=path.resolve(import.meta.dirname,'..');
const exe=process.platform==='win32'?'.exe':'';
const source=path.join(root,'native','target','release','deltamod-gpuix-host'+exe);
const out=path.join(root,'dist');
if(!fs.existsSync(source)) throw new Error('Build the release Rust host first.');
fs.mkdirSync(out,{recursive:true});
fs.copyFileSync(source,path.join(out,'deltamod-gpuix-host'+exe));
if(process.platform!=='win32') fs.chmodSync(path.join(out,'deltamod-gpuix-host'),0o755);
