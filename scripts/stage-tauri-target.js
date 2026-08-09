const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const target = process.argv[2] || process.env.TAURI_BUILD_TARGET || process.env.RUST_TARGET;
const targets = new Set(['x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin']);
if (!targets.has(target)) throw new Error('Target must be x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu, x86_64-apple-darwin, or aarch64-apple-darwin.');

const workspace = path.join(root, 'native');
const tauri = path.join(root, 'src-tauri');
const binaries = path.join(tauri, 'binaries');
const crates = ['hash-worker', 'security-worker', 'copy-worker', 'patch-plan-worker', 'patch-transaction-worker'];
const exe = target.includes('windows') ? '.exe' : '';
const cargo = process.env.CARGO || (process.platform === 'win32'
  ? path.join(process.env.USERPROFILE || '', '.cargo', 'bin', 'cargo.exe')
  : 'cargo');
const result = spawnSync(cargo, ['build', '--release', '--locked', '--target', target, '--manifest-path', path.join(workspace, 'Cargo.toml'), ...crates.flatMap(crate => ['--package', `deltamod-${crate}`])], { cwd: root, stdio: 'inherit', shell: false });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status);
fs.rmSync(binaries, { recursive: true, force: true });
fs.mkdirSync(binaries, { recursive: true });
for (const crate of crates) {
  const source = path.join(workspace, 'target', target, 'release', `deltamod-${crate}${exe}`);
  if (!fs.existsSync(source)) throw new Error(`Missing release binary: ${source}`);
  const destination = path.join(binaries, `deltamod-${crate}-${target}${exe}`);
  fs.copyFileSync(source, destination);
  if (!exe) fs.chmodSync(destination, 0o755);
}
console.log(`Staged ${crates.length} Tauri sidecars for ${target}.`);
