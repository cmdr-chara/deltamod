// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { dialogCount } from './modal-state.mjs';
import { parseIntent } from './deep-links.mjs';

export const MAX_HANDOFF_BYTES = 8192;
export const MAX_PENDING_HANDOFFS = 16;
const MAX_SESSION_REQUESTS = 1024;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);

export function protocolLink(raw) {
  if (typeof raw !== 'string' || Buffer.byteLength(raw, 'utf8') > MAX_HANDOFF_BYTES
    || !/^deltamod-community:\/\//i.test(raw) || /[\u0000-\u0020\u007f\\]/.test(raw)
    || /%(?![0-9a-f]{2})/i.test(raw)) throw new Error('Invalid protocol handoff.');
  // This is only the envelope check. The production Rust parser remains the
  // authority for commands, provider hosts, file IDs, and query parameters.
  let url;
  try { url = new URL(raw); } catch { throw new Error('Invalid protocol handoff.'); }
  if (url.username || url.password || url.port || url.hash) throw new Error('Invalid protocol handoff.');
  return raw;
}

export function archivePath(raw, platform = process.platform) {
  if (typeof raw !== 'string' || !raw || Buffer.byteLength(raw, 'utf8') > 4096
    || /[\u0000-\u001f\u007f]/.test(raw)) throw new Error('Invalid archive handoff.');
  if (/^file:/i.test(raw)) {
    const url = new URL(raw);
    if (url.hostname || url.username || url.password || url.port || url.search || url.hash) {
      throw new Error('Only local archive files may be handed off.');
    }
    raw = fileURLToPath(url, { windows: platform === 'win32' });
  }
  if (platform === 'win32' && (!/^[a-z]:[\\/]/i.test(raw) || raw.slice(2).includes(':'))) {
    throw new Error('Archive handoffs require a drive-rooted path without alternate streams.');
  }
  const paths = platform === 'win32' ? path.win32 : path.posix;
  if (!paths.isAbsolute(raw) || /^[\\/]{2}/.test(raw) || /[\u0000-\u001f\u007f]/.test(raw)
    || paths.extname(raw).toLowerCase() !== '.modarchive') throw new Error('Only local .modarchive files may be handed off.');
  return paths.normalize(raw);
}

export function normalizeHandoff(value, platform = process.platform) {
  if (!record(value) || Object.keys(value).length !== 2 || !Object.hasOwn(value, 'kind')
    || !Object.hasOwn(value, 'value')) throw new Error('Invalid desktop handoff.');
  let raw = value.value;
  if (value.kind === 'preview') parseIntent(raw);
  else if (value.kind === 'protocol') raw = protocolLink(raw);
  else if (value.kind === 'archive') raw = archivePath(raw, platform);
  else throw new Error('Unsupported desktop handoff.');
  return Object.freeze({ kind: value.kind, value: raw });
}

/** Bounded inbox, not an execution queue. Enqueuing never opens a dialog, invokes
 * Rust, reads the archive, navigates, or mutates a game. Review is a user action. */
export class HandoffInbox {
  constructor() {
    this.listeners = new Set();
    this.seen = new Set();
    this.disposed = false;
    this.state = { items: [], reviewing: false, error: '' };
    this.subscribe = listener => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
    this.getSnapshot = () => this.state;
  }
  update(patch) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach(listener => listener());
  }
  receive(values) {
    if (this.disposed || !Array.isArray(values) || values.length > MAX_PENDING_HANDOFFS) {
      throw new Error('Desktop handoff inbox is unavailable.');
    }
    // Validate the whole batch before accepting any of it.
    const fresh = new Map();
    for (const value of values) {
      const item = normalizeHandoff(value);
      const id = createHash('sha256').update(JSON.stringify(item)).digest('hex');
      if (!this.seen.has(id)) fresh.set(id, Object.freeze({ id, ...item }));
    }
    if (this.state.items.length + fresh.size > MAX_PENDING_HANDOFFS
      || this.seen.size + fresh.size > MAX_SESSION_REQUESTS) throw new Error('Desktop handoff inbox is full.');
    for (const id of fresh.keys()) this.seen.add(id);
    if (fresh.size) this.update({ items: [...this.state.items, ...fresh.values()], error: '' });
    // Accepted for review is deliberately not installation success.
    return { accepted: fresh.size, duplicate: values.length - fresh.size };
  }
  discard(id) {
    if (!this.state.reviewing) this.update({ items: this.state.items.filter(item => item.id !== id), error: '' });
  }
  async review(features, managed, model) {
    if (this.disposed || dialogCount() > 0 || this.state.reviewing || !this.state.items.length
      || model.state.loading || model.state.saving || features.state.saving || managed.state.busy || managed.state.loading
      || managed.state.protocol.status !== 'idle' || features.state.pendingLink
      || features.state.detail.status !== 'idle') return false;
    const item = this.state.items[0];
    this.update({ reviewing: true, error: '' });
    try {
      const ok = item.kind === 'preview' ? features.reviewLink(item.value)
        : item.kind === 'protocol' ? await managed.reviewProtocol(item.value)
          : managed.reviewArchive(item.value);
      if (!ok) throw new Error('The request could not be reviewed. Check the request error before retrying.');
      this.update({ items: this.state.items.filter(value => value.id !== item.id), reviewing: false });
      return true;
    } catch (error) {
      this.update({ reviewing: false, error: error instanceof Error ? error.message : String(error) });
      return false;
    }
  }
  dispose() { this.disposed = true; this.listeners.clear(); this.seen.clear(); this.state = { items: [], reviewing: false, error: '' }; }
}
