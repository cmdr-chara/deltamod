// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { spawn } from 'node:child_process';
import { isAbsolute } from 'node:path';

export const PROTOCOL = 1;
export const MAX_REQUEST_BYTES = 64 * 1024;
export const MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
const COMMANDS = new Set(['hello', 'snapshot', 'profile.attach', 'profile.detach', 'installation.select', 'shop.browse', 'preferences.set', 'ui.preferences.get', 'ui.preferences.set', 'theme.preview', 'shop.detail', 'managed.catalog', 'managed.installations', 'managed.game.info', 'managed.game.launch', 'managed.mod.states', 'managed.mod.toggle', 'managed.mod.variant', 'managed.mod.verify', 'managed.mod.repair', 'managed.mod.uninstall', 'managed.restore', 'managed.importArchive', 'managed.patch.run', 'managed.patch.cancel', 'managed.hashes', 'managed.credentials.status', 'managed.credentials.clear', 'managed.nexus.login', 'managed.nexus.cancel']);

/** Private stdio transport. This is not an HTTP server or a Tauri/WebView bridge. */
export class Bridge {
  constructor(child, { timeoutMs = 20000, maxPending = 8 } = {}) {
    this.child = child;
    this.timeoutMs = timeoutMs;
    this.maxPending = maxPending;
    this.pending = new Map();
    this.sequence = 0;
    this.buffer = Buffer.alloc(0);
    this.closed = false;
    this.diagnostics = '';
    this.onData = chunk => this.receive(chunk);
    this.onError = error => this.close(new Error(`Native backend failed: ${error.message}`));
    this.onExit = () => this.close(new Error('Native backend exited. Restart the preview.'));
    this.onStderr = chunk => { this.diagnostics = (this.diagnostics + String(chunk)).slice(-4096); };
    child.stdout.on('data', this.onData);
    child.stderr?.on('data', this.onStderr);
    child.on('error', this.onError);
    child.on('exit', this.onExit);
    child.stdin.on('error', this.onError);
  }

  request(command, args = {}) {
    if (this.closed) return Promise.reject(new Error('Native backend is not connected.'));
    if (!COMMANDS.has(command)) return Promise.reject(new Error('Unsupported backend command.'));
    if (!args || Array.isArray(args) || typeof args !== 'object') return Promise.reject(new Error('Object arguments required.'));
    if (this.pending.size >= this.maxPending) return Promise.reject(new Error('Native backend is busy. Try again.'));
    const id = ++this.sequence;
    let frame;
    try { frame = JSON.stringify({ v: PROTOCOL, id, command, args }) + '\n'; }
    catch { return Promise.reject(new Error('Arguments are not serializable.')); }
    if (Buffer.byteLength(frame) > MAX_REQUEST_BYTES) return Promise.reject(new Error('Request is too large.'));
    return new Promise((resolve, reject) => {
      const timeoutMs = command === 'managed.nexus.login' ? 330000 : this.timeoutMs;
      const timer = setTimeout(() => {
        // A timeout leaves completion unknown. Close, rather than silently queue
        // more work or retry a command which may already have changed preferences.
        this.close(new Error('Native backend timed out. Restart the preview.'));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(frame, error => { if (error) this.onError(error); });
    });
  }

  receive(chunk) {
    if (this.closed) return;
    this.buffer = Buffer.concat([this.buffer, Buffer.from(chunk)]);
    let end;
    while ((end = this.buffer.indexOf(10)) !== -1) {
      if (end > MAX_RESPONSE_BYTES) return this.close(new Error('Native response exceeded its limit.'));
      const line = this.buffer.subarray(0, end).toString('utf8');
      this.buffer = this.buffer.subarray(end + 1);
      let frame;
      try { frame = JSON.parse(line); }
      catch { return this.close(new Error('Malformed native response.')); }
      if (!frame || frame.v !== PROTOCOL || !Number.isSafeInteger(frame.id) || typeof frame.ok !== 'boolean') {
        return this.close(new Error('Incompatible native response.'));
      }
      const pending = this.pending.get(frame.id);
      if (!pending) return this.close(new Error('Unexpected native response ID.'));
      clearTimeout(pending.timer);
      this.pending.delete(frame.id);
      if (frame.ok) pending.resolve(frame.result);
      else pending.reject(new Error(typeof frame.error === 'string' ? frame.error.slice(0, 512) : 'Native operation failed.'));
    }
    if (this.buffer.length > MAX_RESPONSE_BYTES) this.close(new Error('Native response exceeded its limit.'));
  }

  close(error = new Error('Preview closed.')) {
    if (this.closed) return;
    this.closed = true;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
    this.buffer = Buffer.alloc(0);
    this.child.stdout.removeListener('data', this.onData);
    this.child.stderr?.removeListener('data', this.onStderr);
    // Retain error listeners until exit: late pipe errors must not become
    // unhandled exceptions while the process is being reaped.
    this.child.stdin.end();
    if (this.child.exitCode == null && this.child.signalCode == null) this.child.kill();
  }
}

export function startBridge({ executable, stateRoot, resourcesRoot, sourceProfile, managedDataRoot }, spawnImpl = spawn) {
  if (![executable, stateRoot, resourcesRoot].every(value => typeof value === 'string' && isAbsolute(value))) {
    throw new Error('Absolute backend, state and resource paths are required.');
  }
  if (sourceProfile && !isAbsolute(sourceProfile)) throw new Error('Source profile must be an absolute path.');
  if (managedDataRoot && !isAbsolute(managedDataRoot)) throw new Error('Managed data root must be an absolute path.');
  const args = ['--state-root', stateRoot, '--resources-root', resourcesRoot];
  if (sourceProfile) args.push('--source-profile', sourceProfile);
  if (managedDataRoot) args.push('--managed-data-root', managedDataRoot);
  const child = spawnImpl(executable, args, { shell: false, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  return new Bridge(child);
}
