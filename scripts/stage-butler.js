const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const sevenZip = require('7zip-min');
const { resolveTauriTarget } = require('./lib/tauri-target');

const root = path.resolve(__dirname, '..');
const rustTarget = resolveTauriTarget(process.argv[2]);
const targets = {
  'x86_64-pc-windows-msvc': ['windows-amd64', 'f6d06ff12a7e1c7d4a5bd7465aa000283528e3ae2ec354448454e6fff1f0f744', 'butler.exe', '1099ebacba44c5e781babdc0cc409ba91010e284e9ca000e61753e8aa0e84be2'],
  'x86_64-unknown-linux-gnu': ['linux-amd64', '05b9b0ddf98f9c592ea340302b246ad0e8d5afe4f35ff2d03fd6d7f5591647e6', 'butler', 'f32d1d932528c3a0c4c0471d721dfe0c7c24fb16a0fc4e3e81f5a118e0b6d790'],
  'x86_64-apple-darwin': ['darwin-amd64', 'af8666eb9acba0a44589514cfe587ccdbf02b526c1beb19aacb87897f83c79b0', 'butler', '30f3c79fff5efe34474316402c23cccd9164167b25c05c743be5b130c62cd304'],
  'aarch64-apple-darwin': ['darwin-arm64', 'caf7075f7edb7cabfd7e554f6d7f89a6e0f4fdb0c4f9a661bdb73661b99ade0b', 'butler', 'aa5a9591a81ee968f89f45526d7a961fa96c7370f6e18559046a33dfcc81af96']
};
if (!targets[rustTarget]) throw new Error('A supported Rust target is required for butler staging.');

(async () => {
  const [platform, archiveHash, executable, executableHash] = targets[rustTarget];
  const url = `https://broth.itch.zone/butler/${platform}/15.30.0/archive/default`;
  const response = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(120000) });
  if (!response.ok) throw new Error(`butler acquisition failed with HTTP ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (!bytes.length || bytes.length > 64 * 1024 * 1024) throw new Error('butler archive exceeded its size limit.');
  if (crypto.createHash('sha256').update(bytes).digest('hex') !== archiveHash) throw new Error('butler archive checksum mismatch.');
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-butler-'));
  try {
    const archive = path.join(temporary, 'butler.zip');
    const extracted = path.join(temporary, 'extracted');
    fs.writeFileSync(archive, bytes, { flag: 'wx' });
    fs.mkdirSync(extracted);
    const entries = await sevenZip.list(archive);
    for (const entry of entries) {
      const name = String(entry.name || '').replaceAll('\\', '/');
      if (!name || name.startsWith('/') || /^[A-Za-z]:/.test(name) || name.split('/').includes('..') || /L/i.test(String(entry.attr || '').slice(0, 1))) {
        throw new Error(`Unsafe butler archive entry: ${name || '<empty>'}`);
      }
    }
    await sevenZip.unpack(archive, extracted);
    const source = path.join(extracted, executable);
    if (crypto.createHash('sha256').update(fs.readFileSync(source)).digest('hex') !== executableHash) throw new Error('butler executable checksum mismatch.');
    const destination = path.join(root, 'src-tauri', 'resources', 'third-party', 'butler');
    fs.rmSync(destination, { recursive: true, force: true });
    fs.mkdirSync(destination, { recursive: true });
    for (const entry of fs.readdirSync(extracted)) fs.copyFileSync(path.join(extracted, entry), path.join(destination, entry));
    fs.writeFileSync(path.join(destination, 'provenance.json'), JSON.stringify({
      version: '15.30.0', source: url, archiveSha256: archiveHash,
      executable, sha256: executableHash, license: 'MIT'
    }, null, 2) + '\n');
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
