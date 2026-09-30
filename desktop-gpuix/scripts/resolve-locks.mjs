// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { requireLocks } from './locks.mjs';
const root=path.resolve(fileURLToPath(new URL('..',import.meta.url)));
const repo=path.dirname(root);
const digest=file=>fs.existsSync(file)?createHash('sha256').update(fs.readFileSync(file)).digest('hex'):null;
const protectedLocks=['package-lock.json','src-tauri/Cargo.lock','native/Cargo.lock'].map(file=>path.join(repo,file));
const before=protectedLocks.map(digest);
function run(command,args) {
  const result=spawnSync(command,args,{cwd:root,stdio:'inherit',shell:false,timeout:10*60*1000});
  if(result.error) throw result.error;
  if(result.status!==0) throw new Error(`${command} failed while resolving standalone locks.`);
}
try {
  // Check prerequisites before writing either lock. Never fabricate checksums,
  // copy the Tauri lock and call it standalone, or resolve in the root workspace.
  run('cargo',['--version']);
  const npm=process.env.npm_execpath;
  if(!npm || !fs.existsSync(npm)) throw new Error('Run this through npm run lock:resolve.');
  run(process.execPath,[npm,'install','--package-lock-only','--ignore-scripts','--no-audit','--no-fund','--workspaces=false']);
  run('cargo',['generate-lockfile','--manifest-path',path.join(root,'native','Cargo.toml')]);
  requireLocks(root);
  console.log('Standalone npm and Cargo locks resolved. Review and commit both lock files.');
} finally {
  if(protectedLocks.some((file,index)=>digest(file)!==before[index])) throw new Error('A protected Tauri/root lock changed. Do not publish these inputs.');
}
