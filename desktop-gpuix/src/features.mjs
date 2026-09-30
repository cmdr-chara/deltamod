// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import path from 'node:path';
import { supportedLocale } from './languages.mjs';
import { parseIntent, validModId } from './deep-links.mjs';
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const errorMessage = error => error instanceof Error ? error.message : String(error);
const text = (value, limit = 240) => typeof value === 'string' ? [...value.replace(/[\u0000-\u001f\u007f]/g, '')].slice(0, limit).join('') : '';
export function interfacePreferences(value) {
  if (!object(value) || value.schemaVersion !== 1 || !supportedLocale(value.locale) || typeof value.themeImages !== 'boolean') {
    throw new Error('features.badPreferences');
  }
  return { schemaVersion: 1, locale: value.locale, themeImages: value.themeImages };
}
export function plainDescription(value) {
  if (typeof value !== 'string') return '';
  // Output stays plain text. No DOM, markdown image loader or executable HTML.
  return value.slice(0, 64000)
    .replace(/<(script|style)\b[^>]*>[\s\S]*?<\/\1\s*>/gi, '')
    .replace(/<br\b[^>]*>|<\/(?:p|div|li|h[1-6])\s*>/gi, '\n')
    .replace(/<[^>]*>/g, '')
    .replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos|nbsp);/gi, (match, entity) => {
      const named = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
      if (!entity.startsWith('#')) return named[entity.toLowerCase()] || match;
      const number = entity.toLowerCase().startsWith('#x') ? parseInt(entity.slice(2), 16) : Number(entity.slice(1));
      return Number.isInteger(number) && number > 0 && number <= 0x10ffff && !(number >= 0xd800 && number <= 0xdfff)
        ? String.fromCodePoint(number) : '\ufffd';
    })
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, '')
    .replace(/\n{3,}/g, '\n\n').trim().slice(0, 16000);
}
export function normalizeDetail(value, requestedId) {
  if (!object(value) || !validModId(requestedId) || value.id !== requestedId
    || value.url !== `https://gamebanana.com/mods/${requestedId}` || typeof value.name !== 'string'
    || !Array.isArray(value.files) || value.files.length > 20 || value.files.some(file => !object(file))) {
    throw new Error('features.badDetail');
  }
  const seen = new Set();
  const files = value.files.map(file => {
    if (!validModId(file.id) || seen.has(file.id)
      || (file.bytes !== null && file.bytes !== undefined && (!Number.isSafeInteger(file.bytes) || file.bytes < 0))) {
      throw new Error('features.badDetail');
    }
    seen.add(file.id);
    return { id: file.id, name: text(file.name), version: text(file.version, 80), bytes: file.bytes ?? null };
  });
  return { id: requestedId, name: text(value.name), author: text(value.author), game: text(value.game),
    description: plainDescription(value.description), url: value.url, files, hasContentRatings: value.hasContentRatings === true };
}
function normalizeThemeMetadata(value) {
  if (!object(value) || !Array.isArray(value.credits) || value.credits.length > 32
    || value.credits.some(credit => !object(credit))) throw new Error('features.badTheme');
  const color = input => input === null || (typeof input === 'string' && /^#[0-9a-f]{6}$/i.test(input));
  if (!color(value.accent) || !color(value.soulColor)
    || (value.bootSyncTime !== null && (!Number.isFinite(value.bootSyncTime)
      || value.bootSyncTime < 0 || value.bootSyncTime > 3600))) throw new Error('features.badTheme');
  return { name: text(value.name), description: text(value.description, 4096),
    musicTrack: text(value.musicTrack), accent: value.accent, soulColor: value.soulColor,
    bootSyncTime: value.bootSyncTime, videoHasAudio: value.videoHasAudio === true,
    credits: value.credits.map(credit => ({ name: text(credit.name), role: text(credit.role) })) };
}
export function normalizeTheme(value, requestedId) {
  if (!object(value) || value.themeId !== requestedId || !Object.hasOwn(value, 'imagePath')
    || (value.imagePath !== null && (typeof value.imagePath !== 'string' || value.imagePath.length > 16384
      || /[\u0000-\u001f]/.test(value.imagePath) || !path.isAbsolute(value.imagePath) || !/\.(png|jpe?g|webp)$/i.test(value.imagePath)))) {
    throw new Error('features.badTheme');
  }
  return { themeId: requestedId, imagePath: value.imagePath, hasVideo: value.hasVideo === true, hasAudio: value.hasAudio === true,
    ...(Object.hasOwn(value, 'metadata') ? { metadata: normalizeThemeMetadata(value.metadata) } : {}) };
}
const idleDetail = () => ({ status: 'idle', id: '', value: null, error: '' });
const idleTheme = () => ({ status: 'idle', id: '', value: null, error: '' });

/** Presentation state composes with AppModel. It never changes the source profile
 * contract. Requests are event-driven, and closed views discard late responses. */
export class DesktopFeatures {
  constructor(model, bridge) {
    this.model = model;
    this.bridge = bridge;
    this.listeners = new Set();
    this.disposed = false;
    this.detailSequence = 0;
    this.themeSequence = 0;
    this.lastEpoch = model.state.profileEpoch;
    this.lastGame = model.state.shop.gameId;
    this.lastTheme = model.state.snapshot?.preferences.themeId;
    this.state = { preferences: { schemaVersion: 1, locale: 'en', themeImages: false },
      saving: false, error: '', detail: idleDetail(), theme: idleTheme(), pendingLink: null };
    this.subscribe = listener => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
    this.getSnapshot = () => this.state;
    this.unsubscribeModel = model.subscribe(() => {
      const current = model.state;
      if (current.profileEpoch !== this.lastEpoch || current.shop.gameId !== this.lastGame) {
        this.lastEpoch = current.profileEpoch;
        this.lastGame = current.shop.gameId;
        this.closeDetail();
        this.update({ pendingLink: null });
      }
      const themeId = current.snapshot?.preferences.themeId;
      if (themeId !== this.lastTheme) {
        this.lastTheme = themeId;
        this.themeSequence++;
        this.update({ theme: idleTheme() });
      }
    });
  }
  update(patch) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach(listener => listener());
  }
  async initialize() {
    const preferences = interfacePreferences(await this.bridge.request('ui.preferences.get'));
    this.update({ preferences });
    return preferences;
  }
  async savePreferences(patch) {
    if (this.disposed || this.state.saving || this.model.state.loading) return false;
    const next = interfacePreferences({ ...this.state.preferences, ...patch });
    this.update({ saving: true, error: '' });
    try {
      const saved = interfacePreferences(await this.bridge.request('ui.preferences.set', next));
      if (saved.locale !== next.locale || saved.themeImages !== next.themeImages) throw new Error('features.notSaved');
      if (this.disposed) return false;
      this.update({ preferences: saved, saving: false });
      if (!saved.themeImages) { this.themeSequence++; this.update({ theme: idleTheme() }); }
      return true;
    } catch (error) { this.update({ saving: false, error: errorMessage(error) }); return false; }
  }
  async openDetail(id) {
    if (this.disposed || this.model.state.loading) return false;
    if (!validModId(id)) { this.update({ error: 'links.invalid' }); return false; }
    if (this.state.detail.id === id && ['loading', 'ready'].includes(this.state.detail.status)) return true;
    const sequence = ++this.detailSequence;
    this.update({ detail: { status: 'loading', id, value: null, error: '' } });
    try {
      const value = normalizeDetail(await this.bridge.request('shop.detail', { id }), id);
      if (sequence !== this.detailSequence || this.disposed) return false;
      this.update({ detail: { status: 'ready', id, value, error: '' } });
      return true;
    } catch (error) {
      if (sequence === this.detailSequence) this.update({ detail: { status: 'error', id, value: null, error: errorMessage(error) } });
      return false;
    }
  }
  closeDetail() { this.detailSequence++; this.update({ detail: idleDetail() }); }
  async loadTheme(id) {
    if (this.disposed || !this.state.preferences.themeImages || !this.model.state.snapshot?.themes.some(theme => theme.id === id)) return false;
    if (this.state.theme.id === id && ['loading', 'ready', 'error'].includes(this.state.theme.status)) return true;
    const sequence = ++this.themeSequence;
    this.update({ theme: { status: 'loading', id, value: null, error: '' } });
    try {
      const value = normalizeTheme(await this.bridge.request('theme.preview', { themeId: id }), id);
      if (sequence !== this.themeSequence || this.disposed || !this.state.preferences.themeImages) return false;
      this.update({ theme: { status: 'ready', id, value, error: '' } });
      return true;
    } catch (error) {
      if (sequence === this.themeSequence) this.update({ theme: { status: 'error', id, value: null, error: errorMessage(error) } });
      return false;
    }
  }
  async applyThemeColor() {
    const selected = this.model.state.snapshot?.preferences.themeId;
    const theme = this.state.theme;
    if (this.disposed || this.model.state.loading || this.model.state.saving || this.state.saving
      || theme.status !== 'ready' || theme.id !== selected || !theme.value?.metadata?.accent) return false;
    try {
      await this.model.savePreferences({ accent: theme.value.metadata.accent });
      // The model only adopts preferences acknowledged by the native host.
      return !this.disposed && this.model.state.snapshot?.preferences.accent === theme.value.metadata.accent;
    } catch (error) { this.update({ error: errorMessage(error) }); return false; }
  }
  retryTheme() {
    const id = this.model.state.snapshot?.preferences.themeId;
    this.themeSequence++;
    this.update({ theme: idleTheme() });
    if (id) return this.loadTheme(id);
    return Promise.resolve(false);
  }
  reviewLink(raw) {
    if (this.disposed) return false;
    try {
      const intent = parseIntent(raw);
      if (intent.kind === 'browse' && !this.model.state.snapshot?.games.some(game => game.id === intent.gameId && game.gamebanana)) {
        throw new Error('links.unsupportedGame');
      }
      this.update({ pendingLink: intent, error: '' });
      return true;
    } catch (error) { this.update({ pendingLink: null, error: errorMessage(error) }); return false; }
  }
  dismissLink() { this.update({ pendingLink: null }); }
  applyLink() {
    const intent = this.state.pendingLink;
    if (this.disposed || !intent || this.model.state.loading || this.model.state.saving || this.model.state.shop.status === 'loading') return false;
    if (intent.kind === 'browse' && !this.model.state.snapshot?.games.some(game => game.id === intent.gameId && game.gamebanana)) {
      this.update({ pendingLink: null, error: 'links.unsupportedGame' });
      return false;
    }
    this.update({ pendingLink: null });
    if (intent.kind === 'screen') this.model.navigate(intent.route);
    else if (intent.kind === 'mod') void this.openDetail(intent.id);
    else {
      this.model.setShopGame(intent.gameId);
      this.model.navigate('shop');
      void this.model.browse(intent.query);
    }
    return true;
  }
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.detailSequence++;
    this.themeSequence++;
    this.unsubscribeModel();
    this.listeners.clear();
    this.state = { ...this.state, detail: idleDetail(), theme: idleTheme(), pendingLink: null };
  }
}
