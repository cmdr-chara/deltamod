// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createHash, createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { normalizeHandoff, MAX_PENDING_HANDOFFS } from './handoffs.mjs';

const MAX_FRAME = 160 * 1024;
const TIMEOUT = 3000;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const hex = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const mac = (key, direction, value) => createHmac('sha256', Buffer.from(key, 'hex'))
  .update(direction + '\n').update(JSON.stringify(value)).digest('hex');
const authentic = (expected, value) => hex(value)
  && timingSafeEqual(Buffer.from(value, 'hex'), Buffer.from(expected, 'hex'));

function canonicalPath(value) {
  const absolute = path.resolve(value);
  // Resolve existing parents so symlink aliases do not open a second writer.
  let parent = absolute;
  const suffix = [];
  while (!fs.existsSync(parent)) {
    suffix.unshift(path.basename(parent));
    const next = path.dirname(parent);
    if (next === parent) throw new Error('Desktop state directory has no existing parent.');
    parent = next;
  }
  const canonical = path.join(fs.realpathSync(parent), ...suffix);
  return process.platform === 'win32' ? canonical.toLowerCase() : canonical;
}

function privateKey(identity) {
  const uid = typeof process.getuid === 'function' ? process.getuid() : null;
  const user = createHash('sha256').update(os.homedir()).update(String(uid)).digest('hex').slice(0, 20);
  // Separate from the native-owned state directory: do not break its ownership
  // marker or write a file to a source profile merely to claim the instance.
  const directory = path.join(os.tmpdir(), `deltamod-gpuix-ipc-${user}`);
  try { fs.mkdirSync(directory, { mode: 0o700 }); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
  const metadata = fs.lstatSync(directory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()
    || (uid !== null && (metadata.uid !== uid || (metadata.mode & 0o077) !== 0))) {
    throw new Error('Desktop handoff directory is not private.');
  }
  const filename = path.join(directory, `${identity}.key`);
  let descriptor;
  try {
    descriptor = fs.openSync(filename, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600);
    fs.writeFileSync(descriptor, randomBytes(32).toString('hex'));
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
  } finally { if (descriptor !== undefined) fs.closeSync(descriptor); }
  const read = fs.openSync(filename, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
  try {
    const info = fs.fstatSync(read);
    const link = fs.lstatSync(filename);
    if (!info.isFile() || link.isSymbolicLink() || info.size !== 64 || info.nlink !== 1
      || (uid !== null && (info.uid !== uid || (info.mode & 0o077) !== 0))) {
      throw new Error('Desktop handoff key is not a private regular file.');
    }
    const value = fs.readFileSync(read, 'utf8');
    if (!hex(value)) throw new Error('Desktop handoff key is invalid.');
    return value;
  } finally { fs.closeSync(read); }
}

function readFrame(socket, receive, failure) {
  let bytes = 0;
  const chunks = [];
  let finished = false;
  const deadline = setTimeout(() => fail(new Error('Desktop handoff timed out.')), TIMEOUT);
  deadline.unref();
  const fail = error => {
    if (finished) return;
    finished = true;
    clearTimeout(deadline);
    failure(error);
    socket.destroy();
  };
  socket.on('error', fail);
  socket.on('data', chunk => {
    if (finished) return;
    bytes += chunk.length;
    if (bytes > MAX_FRAME) return fail(new Error('Desktop handoff exceeds the message limit.'));
    chunks.push(chunk);
    const end = chunk.indexOf(10);
    if (end === -1) return;
    const data = Buffer.concat(chunks);
    if (data.indexOf(10) !== data.length - 1) return fail(new Error('Desktop handoff must contain exactly one message.'));
    try {
      const value = JSON.parse(data.subarray(0, -1).toString('utf8'));
      finished = true;
      clearTimeout(deadline);
      receive(value);
    } catch (error) {
      finished = false;
      fail(error);
    }
  });
  socket.once('close', () => {
    if (!finished) fail(new Error('Desktop handoff was not acknowledged. Its completion is unknown.'));
    clearTimeout(deadline);
  });
}

function forward(port, token, items) {
  return new Promise((resolve, reject) => {
    const id = randomBytes(32).toString('hex');
    const socket = net.createConnection({ host: '127.0.0.1', port });
    readFrame(socket, value => {
      socket.end();
      if (!record(value) || Object.keys(value).length !== 5 || value.id !== id || value.ok !== true
        || !authentic(mac(token, 'receipt', [value.id, value.ok, value.accepted, value.duplicate]), value.mac)
        || !Number.isSafeInteger(value.accepted) || !Number.isSafeInteger(value.duplicate)
        || value.accepted < 0 || value.duplicate < 0 || value.accepted + value.duplicate !== items.length) {
        reject(new Error('The running application did not accept the handoff. Nothing will be retried automatically.'));
      } else resolve({ primary: false, accepted: value.accepted, dispose() {} });
    }, reject);
    socket.once('connect', () => socket.write(JSON.stringify({ version: 1, id, items,
      mac: mac(token, 'request', [1, id, items]) }) + '\n'));
  });
}

/** Kernel-held loopback listener, with a private per-user capability. A fixed
 * workspace-derived port avoids stale PID/lock reclamation races. A collision or
 * unknown response fails closed, never starts a second backend or retries an
 * operation. Requests and receipts use domain-separated HMACs: the private key
 * is never sent to a process occupying the port. This is not an HTTP or frontend
 * network bridge. */
export async function startSingleInstance(options, items, receive) {
  if (!Array.isArray(items) || items.length > MAX_PENDING_HANDOFFS) throw new Error('Too many desktop handoffs.');
  const normalized = items.map(value => normalizeHandoff(value));
  if (options.benchmarkFile && normalized.length) throw new Error('A benchmark cannot receive desktop handoffs.');
  // Benchmark mode must not bypass writer exclusion for the same managed root.
  const identity = createHash('sha256').update(canonicalPath(options.managedDataRoot || options.stateRoot)).digest('hex');
  const token = privateKey(identity);
  const port = 32768 + (parseInt(identity.slice(0, 8), 16) % 28000);
  const sockets = new Set();
  const server = net.createServer(socket => {
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
    readFrame(socket, value => {
      if (!record(value) || Object.keys(value).length !== 4 || value.version !== 1 || !hex(value.id)
        || !authentic(mac(token, 'request', [value.version, value.id, value.items]), value.mac)
        || !Array.isArray(value.items) || value.items.length > MAX_PENDING_HANDOFFS) {
        socket.destroy();
        return;
      }
      try {
        const receipt = receive(value.items.map(item => normalizeHandoff(item)));
        socket.end(JSON.stringify({ id: value.id, ok: true, ...receipt,
          mac: mac(token, 'receipt', [value.id, true, receipt.accepted, receipt.duplicate]) }) + '\n');
      } catch {
        // Do not disclose request contents, paths, or the IPC capability.
        socket.end(JSON.stringify({ id: value.id, ok: false }) + '\n');
      }
    }, () => {});
  });
  server.maxConnections = 16;
  try {
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen({ port, host: '127.0.0.1', exclusive: true }, resolve);
    });
  } catch (error) {
    server.close(() => {});
    if (error.code !== 'EADDRINUSE') throw error;
    // One forwarding attempt only. A lost acknowledgement is not permission to
    // run the request in a fresh instance.
    return forward(port, token, normalized);
  }
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    for (const socket of sockets) socket.destroy();
    server.close();
  };
  server.unref();
  try {
    const receipt = receive(normalized);
    return { primary: true, accepted: receipt.accepted, dispose };
  } catch (error) { dispose(); throw error; }
}
