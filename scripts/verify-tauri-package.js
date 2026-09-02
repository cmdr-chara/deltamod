const fs = require('fs');
const path = require('path');
const root = path.resolve(__dirname, '..');
const artifact = process.argv[2] || process.env.TAURI_ARTIFACT;
const target = process.argv[3] || process.env.TAURI_BUILD_TARGET || process.env.RUST_TARGET;
const unsignedPreview = process.argv.includes('--unsigned');
if (!artifact) throw new Error('Pass the unpacked bundle directory with TAURI_ARTIFACT or argv[2].');
if (!target) throw new Error('Pass the Rust target with TAURI_BUILD_TARGET, RUST_TARGET, or argv[3].');
const bundle = path.resolve(artifact);
if (!fs.statSync(bundle).isDirectory()) throw new Error(`Artifact is not a directory: ${bundle}`);
const required = ['NOTICE.md', 'THIRD_PARTY_NOTICES.md'];
function find(name) { const result = []; const visit = dir => { for (const entry of fs.readdirSync(dir, { withFileTypes: true })) { const file = path.join(dir, entry.name); if (entry.isDirectory()) visit(file); else if (entry.name === name) result.push(file); } }; visit(bundle); return result; }
const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const updateCapable = target === 'x86_64-pc-windows-msvc'
    || target === 'x86_64-apple-darwin'
    || target === 'aarch64-apple-darwin';
const platformConfig = updateCapable
    ? JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', target.includes('apple')
        ? 'tauri.macos.conf.json' : 'tauri.windows.conf.json'), 'utf8'))
    : null;
if (config.bundle.active !== true || config.bundle.createUpdaterArtifacts !== false
    || (updateCapable && platformConfig?.bundle?.createUpdaterArtifacts !== true)) {
    throw new Error('Bundle updater policy does not match the target platform.');
}
if (config.bundle.license !== 'EUPL-1.2') throw new Error('Bundle license metadata is incorrect.');
const expectedExtensions = {
  'x86_64-pc-windows-msvc': ['.exe'],
  'x86_64-unknown-linux-gnu': ['.deb'],
  'x86_64-apple-darwin': ['.dmg'],
  'aarch64-apple-darwin': ['.dmg']
}[target];
if (!expectedExtensions) throw new Error(`Unsupported package target: ${target}`);
const packages = [];
const packageExtensions = new Set(expectedExtensions);
const visitPackages = dir => { for (const entry of fs.readdirSync(dir, { withFileTypes: true })) { const file = path.join(dir, entry.name); if (entry.isDirectory()) visitPackages(file); else if (packageExtensions.has(path.extname(entry.name).toLowerCase()) && fs.statSync(file).size >= 1024 * 1024) packages.push(file); } };
visitPackages(bundle);
const foundExtensions = new Set(packages.map(file => path.extname(file).toLowerCase()));
for (const extension of expectedExtensions) {
  if (!foundExtensions.has(extension)) throw new Error(`Bundle is missing the expected ${extension} artifact for ${target}.`);
}
if (packages.some(file => !path.basename(file).includes(config.version))) throw new Error(`A package filename does not contain version ${config.version}.`);
const updaterSignatures = [];
const visitSignatures = dir => { for (const entry of fs.readdirSync(dir, { withFileTypes: true })) { const file = path.join(dir, entry.name); if (entry.isDirectory()) visitSignatures(file); else if (entry.name.endsWith('.sig')) updaterSignatures.push(file); } };
visitSignatures(bundle);
if (unsignedPreview) {
    if (updaterSignatures.length !== 0) {
        throw new Error('Unsigned preview package must not contain automatic updater signatures.');
    }
} else if (updateCapable) {
    if (updaterSignatures.length !== 1) throw new Error('Update-capable package must contain exactly one Tauri signature.');
    const updaterArtifact = updaterSignatures[0].slice(0, -4);
    if (!fs.existsSync(updaterArtifact) || fs.statSync(updaterArtifact).size > 512 * 1024 * 1024) {
        throw new Error('Signed updater artifact is missing or exceeds 512 MiB.');
    }
} else if (updaterSignatures.length !== 0) {
    throw new Error('Linux .deb package must not contain automatic updater signatures.');
}
const hasVisibleContents = required.every(file => find(file).length);
if (hasVisibleContents) {
  const tools = target === 'aarch64-apple-darwin' ? ['G3MTool'] : ['G3MTool', 'UndertaleModCli'];
  for (const name of tools) if (!find(name).length && !find(`${name}.exe`).length) throw new Error(`Bundle is missing trusted patch tool ${name}.`);
  for (const name of ['deltamod-hash-worker', 'deltamod-security-worker', 'deltamod-copy-worker', 'deltamod-patch-plan-worker', 'deltamod-patch-transaction-worker']) if (!find(name).length && !find(`${name}.exe`).length) throw new Error(`Bundle is missing sidecar ${name}.`);
  if (!find('butler').length && !find('butler.exe').length) throw new Error('Bundle is missing checksum-verified butler.');
  if (!find('provenance.json').length) throw new Error('Bundle is missing butler provenance.');
}
console.log(`Verified ${unsignedPreview ? 'unsigned preview ' : ''}${packages.length} Tauri package artifact(s)${hasVisibleContents ? ' and unpacked contents' : ''}: ${bundle}`);
