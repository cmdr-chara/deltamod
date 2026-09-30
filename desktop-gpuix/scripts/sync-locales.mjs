// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { languages } from '../src/languages.mjs';
import { parseCatalogue, reuseCatalogue } from './source-catalogues.mjs';
const root = path.resolve(import.meta.dirname, '../..');
const directory = fs.realpathSync(path.join(root, 'web/langs'));
const sources = {}, digests = {};
for (const { id } of languages) {
  const folder = path.join(directory, id);
  const filename = path.join(folder, 'language.json');
  const metadata = fs.lstatSync(filename);
  if (fs.lstatSync(folder).isSymbolicLink() || metadata.isSymbolicLink() || !metadata.isFile()
    || metadata.size > 256 * 1024 || fs.realpathSync(filename) !== filename) throw new Error(`Unsafe source language file: ${id}`);
  const source = fs.readFileSync(filename, 'utf8');
  sources[id] = parseCatalogue(source);
  digests[id] = createHash('sha256').update(source).digest('hex');
}
const dictionaries = Object.fromEntries(languages.map(({id}) => [id, reuseCatalogue(sources.en, sources[id])]));
const out = path.join(root, 'desktop-gpuix/src/generated');
fs.mkdirSync(out, { recursive: true });
// JSON.parse prevents object-literal prototype semantics. Never eval a source file.
const payload = '// Generated from web/langs/*/language.json. Do not edit.\n'
  + 'export const sourceDigests = ' + JSON.stringify(digests) + ';\n'
  + 'export const sourceDictionaries = JSON.parse(' + JSON.stringify(JSON.stringify(dictionaries)) + ');\n';
const output = path.join(out, 'source-locales.mjs');
if (!fs.existsSync(output) || fs.readFileSync(output, 'utf8') !== payload) fs.writeFileSync(output, payload);
console.log('Reused existing source strings: ' + languages.map(({id}) => `${id}=${Object.keys(dictionaries[id]).length}`).join(', '));
