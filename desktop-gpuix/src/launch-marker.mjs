// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const LAUNCH_MARKER = 'deltamod-community-open-v1\n';
const DIRECTORY = 'Deltamod Community CLI';
const records = new WeakMap();
const invalid = () => new Error('The Deltamod CLI launch marker is not trusted.');

export function launchMarkerPath(raw, platform = process.platform) {
  if (typeof raw !== 'string' || !raw || Buffer.byteLength(raw) > 4096 || /[\x00-\x1f\x7f]/.test(raw)) throw invalid();
  if (/^file:/i.test(raw)) {
    const url = new URL(raw);
    if (url.hostname || url.username || url.password || url.port || url.search || url.hash) throw invalid();
    raw = fileURLToPath(url, { windows: platform === 'win32' });
  }
  const paths = platform === 'win32' ? path.win32 : path.posix;
  if (!paths.isAbsolute(raw) || /^[\\/]{2}/.test(raw) || /[\x00-\x1f\x7f]/.test(raw)
    || paths.extname(raw).toLowerCase() !== '.deltamod-open'
    || (platform === 'win32' && (!/^[a-z]:[\\/]/i.test(raw) || raw.slice(2).includes(':')))) throw invalid();
  return paths.normalize(raw);
}

function owned(metadata) {
  return typeof process.getuid !== 'function'
    || (metadata.uid === process.getuid() && (metadata.mode & 0o022) === 0);
}
function same(a, b) { return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs; }
function content(descriptor) {
  const bytes = Buffer.alloc(Buffer.byteLength(LAUNCH_MARKER) + 1);
  const count = fs.readSync(descriptor, bytes, 0, bytes.length, 0);
  return count === bytes.length - 1 && bytes.subarray(0, count).toString('utf8') === LAUNCH_MARKER;
}

/** A marker can only request foregrounding. It never supplies commands, a data
 * root, an import, or credentials. Keep the descriptor until acknowledged so a
 * failed launch leaves the caller's marker intact. tempRoot is only a test seam. */
export function readLaunchMarker(raw, tempRoot = os.tmpdir()) {
  const filename = launchMarkerPath(raw);
  const root = path.join(fs.realpathSync(tempRoot), DIRECTORY);
  const parent = fs.lstatSync(root);
  if (!parent.isDirectory() || parent.isSymbolicLink() || !owned(parent)) throw invalid();
  // Canonical temp aliases (notably /var on macOS) are permitted, not a marker
  // under an arbitrary directory or a linked CLI handoff directory.
  if (fs.realpathSync(path.dirname(filename)) !== root) throw invalid();
  const original = fs.lstatSync(filename);
  if (!original.isFile() || original.isSymbolicLink() || original.nlink !== 1
    || original.size !== Buffer.byteLength(LAUNCH_MARKER) || !owned(original)) throw invalid();
  const descriptor = fs.openSync(filename, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
  try {
    const actual = fs.fstatSync(descriptor);
    if (!same(actual, original) || !content(descriptor) || fs.realpathSync(filename) !== path.join(root, path.basename(filename))) throw invalid();
    const ticket = Object.freeze({ path: path.join(root, path.basename(filename)) });
    records.set(ticket, { descriptor, original, root });
    return ticket;
  } catch (error) { fs.closeSync(descriptor); throw error; }
}

/** Acknowledgement means delivery, not OS permission to steal focus. Never call
 * this after an uncertain socket receipt or a failed primary initialization. */
export function acknowledgeLaunchMarker(ticket) {
  const record = records.get(ticket);
  if (!record) return false;
  try {
    const parent = fs.lstatSync(record.root);
    const current = fs.lstatSync(ticket.path);
    if (!parent.isDirectory() || parent.isSymbolicLink() || !owned(parent)
      || !current.isFile() || current.isSymbolicLink() || current.nlink !== 1
      || !same(current, record.original) || !same(fs.fstatSync(record.descriptor), record.original)
      || fs.realpathSync(ticket.path) !== ticket.path || !content(record.descriptor)) throw invalid();
    fs.unlinkSync(ticket.path);
    return true;
  } finally { abandonLaunchMarker(ticket); }
}

export function abandonLaunchMarker(ticket) {
  const record = records.get(ticket);
  if (!record) return;
  records.delete(ticket);
  fs.closeSync(record.descriptor);
}
