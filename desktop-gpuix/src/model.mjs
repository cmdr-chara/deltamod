// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const ROUTES = ['home', 'library', 'installations', 'shop', 'themes', 'settings'];
const clean = (value, fallback = '', max = 240) => typeof value === 'string'
  ? value.replace(/[\u0000-\u001f\u007f]/g, '').slice(0, max) : fallback;
export function validColor(value) { return typeof value === 'string' && /^#[\da-f]{6}$/i.test(value); }
export function preferences(value = {}) {
  return { themeId: clean(value.themeId, 'base', 64), accent: validColor(value.accent) ? value.accent : '#cd4451',
    reducedMotion: value.reducedMotion !== false, opaque: value.opaque !== false };
}
export function normalizeSnapshot(value) {
  if (!value || typeof value !== 'object' || !Array.isArray(value.installations) || !Array.isArray(value.mods) || !Array.isArray(value.themes)) {
    throw new Error('Native snapshot does not match the preview contract.');
  }
  return {
    sourceAttached: value.sourceAttached === true,
    preferences: preferences(value.preferences),
    installations: value.installations.slice(0, 128).map((item, i) => ({
      id: clean(item.id, String(i), 80), name: clean(item.name, 'Installation'), gameId: clean(item.gameId),
      path: clean(item.path, '', 4096), current: item.current === true, available: typeof item.available === 'boolean' ? item.available : null
    })),
    mods: value.mods.slice(0, 500).map((item, i) => ({
      id: clean(item.id, String(i)), name: clean(item.name, 'Unnamed mod'),
      description: clean(item.description, '', 600), enabled: typeof item.enabled === 'boolean' ? item.enabled : null,
      format: clean(item.format, 'unknown', 40)
    })),
    themes: value.themes.slice(0, 128).map(item => ({
      id: clean(item.id, 'base', 64), name: clean(item.name, 'Theme'),
      accent: validColor(item.accent) ? item.accent : '#cd4451'
    })),
    warnings: Array.isArray(value.warnings) ? value.warnings.slice(0, 12).map(item => clean(item, '', 512)) : [],
    readOnly: true
  };
}
export function normalizeShop(value) {
  if (!value || !Array.isArray(value.items)) throw new Error('Unexpected Mod Shop response.');
  return { items: value.items.slice(0, 24).map(item => ({
    id: clean(item.id, '', 24), name: clean(item.name, 'Untitled mod'), author: clean(item.author),
    description: clean(item.description, '', 400), url: clean(item.url, '', 256)
  })).filter(item => /^https:\/\/gamebanana\.com\/mods\/[1-9]\d*$/.test(item.url)),
  hasMore: value.hasMore === true };
}
export function filterMods(mods, query, enabledOnly = false) {
  const term = String(query).trim().toLocaleLowerCase();
  return mods.filter(mod => (!enabledOnly || mod.enabled)
    && `${mod.name} ${mod.description}`.toLocaleLowerCase().includes(term));
}

/** UI state is independent of React/GPUIX, so contract and race checks need no GPU. */
export class AppModel {
  constructor(bridge) {
    this.bridge = bridge;
    this.listeners = new Set();
    this.revision = 0;
    this.shopRevision = 0;
    this.disposed = false;
    this.state = { route: 'home', snapshot: null, error: '', loading: false, saving: false,
      shop: { status: 'idle', items: [], error: '', hasMore: false, page: 1, query: '' } };
    this.subscribe = listener => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
    this.getSnapshot = () => this.state;
  }
  update(patch) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach(listener => listener());
  }
  navigate(route) { if (ROUTES.includes(route)) this.update({ route, error: '' }); }
  async initialize() {
    const hello = await this.bridge.request('hello');
    if (!hello || hello.protocol !== 1 || hello.readOnly !== true || !Array.isArray(hello.capabilities) || !hello.capabilities.includes('snapshot')) {
      throw new Error('The native backend is incompatible with this preview.');
    }
    return this.refresh();
  }
  async refresh() {
    const revision = ++this.revision;
    this.update({ loading: true, error: '' });
    try {
      const value = normalizeSnapshot(await this.bridge.request('snapshot'));
      if (revision === this.revision) this.update({ snapshot: value, loading: false });
      return value;
    } catch (error) {
      if (revision === this.revision) this.update({ error: error.message, loading: false });
      throw error;
    }
  }
  async attachProfile(path) {
    if (this.state.loading || this.state.saving) return;
    const revision = ++this.revision;
    this.update({ loading: true, error: '' });
    try {
      const snapshot = normalizeSnapshot(await this.bridge.request('profile.attach', { path }));
      if (revision === this.revision) this.update({ snapshot, loading: false });
    } catch (error) {
      if (revision === this.revision) this.update({ error: error.message, loading: false });
    }
  }
  async savePreferences(patch) {
    if (!this.state.snapshot || this.state.saving) return;
    // Only commit the acknowledged result. No optimistic "saved" indication.
    this.update({ saving: true, error: '' });
    try {
      const next = preferences(await this.bridge.request('preferences.set', { ...this.state.snapshot.preferences, ...patch }));
      this.update({ saving: false, snapshot: { ...this.state.snapshot, preferences: next } });
    } catch (error) { this.update({ saving: false, error: error.message }); }
  }
  async browse(query, page = 1) {
    query = String(query).trim().slice(0, 128);
    page = Math.min(100, Math.max(1, Math.trunc(page) || 1));
    const revision = ++this.shopRevision;
    this.update({ shop: { ...this.state.shop, status: 'loading', query, page, error: '' } });
    try {
      const value = normalizeShop(await this.bridge.request('shop.browse', { query, page }));
      if (revision === this.shopRevision) this.update({ shop: { ...value, status: 'ready', error: '', query, page } });
    } catch (error) {
      if (revision === this.shopRevision) this.update({ shop: { ...this.state.shop, status: 'error', error: error.message } });
    }
  }
  dispose() { this.disposed = true; this.revision++; this.shopRevision++; this.listeners.clear(); this.bridge.close(); }
}
