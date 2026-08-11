const fs = require('fs');
const path = require('path');
const g3mProvenance = require('./lib/g3mtool-provenance');
const undertaleProvenance = require('./lib/undertale-mod-tool-provenance');
const root = path.resolve(__dirname, '..');
const target = process.argv[2] || process.env.TAURI_BUILD_TARGET || process.env.RUST_TARGET;
const map = { 'x86_64-pc-windows-msvc': 'win32-x64', 'x86_64-unknown-linux-gnu': 'linux-x64', 'x86_64-apple-darwin': 'darwin-x64', 'aarch64-apple-darwin': 'darwin-arm64' };
const platformTarget = map[target];
if (!platformTarget) throw new Error('A supported Rust target is required.');
const output = path.join(root, 'src-tauri', 'resources', 'third-party');
g3mProvenance.verifyInstallation(root, g3mProvenance.loadProvenance(root), platformTarget);
if (platformTarget !== 'darwin-arm64') {
  undertaleProvenance.verifyInstallation(root, undertaleProvenance.loadProvenance(root), platformTarget);
}
fs.rmSync(output, { recursive: true, force: true });
fs.mkdirSync(output, { recursive: true });
const sources = [
  [path.join(root, 'tools', 'g3mtool'), path.join(output, 'g3mtool')],
  [path.join(root, 'tools', 'undertale-mod-tool'), path.join(output, 'undertale-mod-tool')]
];
for (const [sourceRoot, destinationRoot] of sources) {
  if (directoryName(sourceRoot) === 'undertale-mod-tool' && platformTarget === 'darwin-arm64') continue;
  const entries = fs.readdirSync(sourceRoot, { withFileTypes: true });
  const candidates = entries.filter(entry => entry.isDirectory() && entry.name === ({ 'win32-x64': 'win-x64', 'linux-x64': 'linux-x64', 'darwin-x64': 'mac-x64', 'darwin-arm64': 'mac-arm64' })[platformTarget]);
  if (candidates.length !== 1) throw new Error(`Expected exactly one verified tool directory for ${platformTarget} in ${sourceRoot}.`);
  fs.cpSync(path.join(sourceRoot, candidates[0].name), path.join(destinationRoot, candidates[0].name), { recursive: true, errorOnExist: true });
}
function directoryName(value) { return path.basename(value); }
for (const file of ['NOTICE.md', 'THIRD_PARTY_NOTICES.md']) fs.copyFileSync(path.join(root, file), path.join(root, 'src-tauri', 'resources', file));
console.log(`Staged checksum-verified third-party resources for ${platformTarget}.`);
