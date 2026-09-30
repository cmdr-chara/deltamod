// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { regularFile } from '../src/runtime-layout.mjs';

export function requireLocks(desktopRoot) {
  const pkg=JSON.parse(fs.readFileSync(path.join(desktopRoot,'package.json'),'utf8'));
  const lock=JSON.parse(fs.readFileSync(regularFile(desktopRoot,'package-lock.json',16*1024*1024),'utf8'));
  if(lock.lockfileVersion!==3 || lock.name!==pkg.name || lock.version!==pkg.version || !lock.packages?.['']) {
    throw new Error('A resolved standalone npm v3 lock matching the GPUIX package is required.');
  }
  for(const [name,version] of Object.entries({...pkg.dependencies,...pkg.devDependencies})) {
    const root=lock.packages[''];
    if((root.dependencies?.[name]??root.devDependencies?.[name])!==version
      || lock.packages[`node_modules/${name}`]?.version!==version) throw new Error(`The standalone npm lock does not pin ${name}.`);
  }
  for(const [name,entry] of Object.entries(lock.packages)) {
    if(!name) continue;
    if(entry.link || typeof entry.resolved!=='string' || !entry.resolved.startsWith('https://')
      || typeof entry.integrity!=='string' || !/^sha(256|384|512)-[A-Za-z0-9+/]+={0,2}$/.test(entry.integrity)) {
      throw new Error(`Unresolved or untrusted npm lock record: ${name}`);
    }
  }
  const cargo=fs.readFileSync(regularFile(desktopRoot,'native/Cargo.lock',16*1024*1024),'utf8');
  if(!/^version = [34]$/m.test(cargo) || !/^name = "deltamod-gpuix-host"$/m.test(cargo)) {
    throw new Error('The standalone GPUIX host Cargo lock is missing or invalid.');
  }
  return lock;
}
