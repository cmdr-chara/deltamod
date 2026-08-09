const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const root = path.resolve(__dirname, '..');
const artifact = process.argv[2] || process.env.TAURI_ARTIFACT;
if (!artifact) throw new Error('Pass the unpacked bundle directory with TAURI_ARTIFACT or argv[2].');
const bundle = path.resolve(artifact);
if (!fs.statSync(bundle).isDirectory()) throw new Error(`Artifact is not a directory: ${bundle}`);
const required = ['NOTICE.md', 'THIRD_PARTY_NOTICES.md'];
function find(name) { const result = []; const visit = dir => { for (const entry of fs.readdirSync(dir, { withFileTypes: true })) { const file = path.join(dir, entry.name); if (entry.isDirectory()) visit(file); else if (entry.name === name) result.push(file); } }; visit(bundle); return result; }
for (const file of required) if (!find(file).length) throw new Error(`Bundle is missing ${file}.`);
for (const name of ['G3MTool', 'UndertaleModCli']) if (!find(name).length && !find(`${name}.exe`).length) throw new Error(`Bundle is missing trusted patch tool ${name}.`);
for (const name of ['deltamod-hash-worker', 'deltamod-security-worker', 'deltamod-copy-worker', 'deltamod-patch-plan-worker', 'deltamod-patch-transaction-worker']) if (!find(name).length && !find(`${name}.exe`).length) throw new Error(`Bundle is missing sidecar ${name}.`);
if (!find('butler').length && !find('butler.exe').length) throw new Error('Bundle is missing checksum-verified butler.');
if (!find('provenance.json').length) throw new Error('Bundle is missing butler provenance.');
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
if (config.bundle.active !== true || config.bundle.createUpdaterArtifacts !== false) throw new Error('Bundle must be active with updater artifacts disabled for unsigned beta.');
if (config.bundle.license !== 'EUPL-1.2') throw new Error('Bundle license metadata is incorrect.');
console.log(`Verified Tauri package contents: ${bundle}`);
