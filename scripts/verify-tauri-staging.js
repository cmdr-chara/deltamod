const fs = require('fs');
const crypto = require('crypto');
const path = require('path');
const root = path.resolve(__dirname, '..');
const target = process.argv[2] || process.env.TAURI_BUILD_TARGET || process.env.RUST_TARGET;
const exe = target && target.includes('windows') ? '.exe' : '';
const toolTarget = {
  'x86_64-pc-windows-msvc': 'win-x64',
  'x86_64-unknown-linux-gnu': 'linux-x64',
  'x86_64-apple-darwin': 'mac-x64',
  'aarch64-apple-darwin': 'mac-arm64'
}[target];
if (!target) throw new Error('RUST_TARGET or a target argument is required.');
if (!toolTarget) throw new Error(`Unsupported target: ${target}`);
const crates = ['hash-worker', 'security-worker', 'copy-worker', 'patch-plan-worker', 'patch-transaction-worker'];
for (const crate of crates) {
  const file = path.join(root, 'src-tauri', 'binaries', `deltamod-${crate}-${target}${exe}`);
  const stat = fs.statSync(file);
  if (!stat.isFile() || stat.size < 1024) throw new Error(`Invalid sidecar: ${file}`);
  if (!exe && (stat.mode & 0o111) === 0) throw new Error(`Sidecar is not executable: ${file}`);
}
for (const file of ['NOTICE.md', 'THIRD_PARTY_NOTICES.md']) fs.statSync(path.join(root, 'src-tauri', 'resources', file));
for (const directory of ['g3mtool', 'undertale-mod-tool']) {
  if (directory === 'undertale-mod-tool' && target === 'aarch64-apple-darwin') continue;
  const rootPath = path.join(root, 'src-tauri', 'resources', 'third-party', directory);
  const candidates = fs.readdirSync(rootPath, { withFileTypes: true }).filter(entry => entry.isDirectory());
  if (candidates.length !== 1) throw new Error(`Expected one staged ${directory} tree.`);
  const license = directory === 'g3mtool' ? ['LICENSE', 'LICENSE.txt'] : ['LICENSE.txt'];
  if (!license.some(file => fs.existsSync(path.join(rootPath, candidates[0].name, file)))) throw new Error(`Staged ${directory} tree has no license file.`);
  const executable = directory === 'g3mtool' ? `G3MTool${exe}` : `UndertaleModCli${exe}`;
  const executablePath = path.join(rootPath, toolTarget, executable);
  const stat = fs.statSync(executablePath);
  if (!stat.isFile() || stat.size < 1024) throw new Error(`Invalid staged patch tool: ${executablePath}`);
}
const butlerRoot = path.join(root, 'src-tauri', 'resources', 'third-party', 'butler');
const butlerProvenance = JSON.parse(fs.readFileSync(path.join(butlerRoot, 'provenance.json'), 'utf8'));
const expectedButler = {
  'x86_64-pc-windows-msvc': ['butler.exe', '1099ebacba44c5e781babdc0cc409ba91010e284e9ca000e61753e8aa0e84be2'],
  'x86_64-unknown-linux-gnu': ['butler', 'f32d1d932528c3a0c4c0471d721dfe0c7c24fb16a0fc4e3e81f5a118e0b6d790'],
  'x86_64-apple-darwin': ['butler', '30f3c79fff5efe34474316402c23cccd9164167b25c05c743be5b130c62cd304'],
  'aarch64-apple-darwin': ['butler', 'aa5a9591a81ee968f89f45526d7a961fa96c7370f6e18559046a33dfcc81af96']
}[target];
if (butlerProvenance.version !== '15.30.0' || butlerProvenance.executable !== expectedButler[0] || butlerProvenance.sha256 !== expectedButler[1] || butlerProvenance.license !== 'MIT') throw new Error('Invalid staged butler provenance.');
const butlerPath = path.join(butlerRoot, expectedButler[0]);
if (crypto.createHash('sha256').update(fs.readFileSync(butlerPath)).digest('hex') !== expectedButler[1]) throw new Error('Staged butler checksum mismatch.');
console.log(`Verified Tauri staging for ${target}.`);
