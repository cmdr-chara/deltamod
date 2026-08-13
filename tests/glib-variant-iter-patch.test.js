const fs = require('node:fs');
const path = require('node:path');

const repoRoot = path.resolve(__dirname, '..');

test('the Tauri GLib dependency uses the patched local source', () => {
  const lockfile = fs.readFileSync(path.join(repoRoot, 'src-tauri', 'Cargo.lock'), 'utf8');
  const glibEntry = lockfile.match(/\[\[package\]\]\nname = "glib"\nversion = "0\.18\.5"\n([\s\S]*?)(?=\n\[\[package\]\]|$)/)?.[1];
  expect(glibEntry).toBeDefined();
  expect(glibEntry).not.toContain('source = "registry+https://github.com/rust-lang/crates.io-index"');

  const variantIter = fs.readFileSync(
    path.join(repoRoot, 'vendor', 'glib-0.18.5-patched', 'src', 'variant_iter.rs'),
    'utf8',
  );
  expect(variantIter).toContain('let mut p: *mut libc::c_char');
  expect(variantIter).toContain('&mut p');
  expect(variantIter).not.toContain('let p: *mut libc::c_char = std::ptr::null_mut();');
});
