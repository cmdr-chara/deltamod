// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const ROUTES = ['home', 'library', 'installations', 'shop', 'themes', 'settings'];
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const clean = (value, fallback = '', max = 240) => typeof value === 'string'
  ? [...value.replace(/[\u0000-\u001f\u007f]/g, '')].slice(0, max).join('') : fallback;
const message = error => error instanceof Error ? error.message : String(error);
const emptyShop = (gameId = '') => ({ status: 'idle', items: [], error: '', hasMore: false, page: 1, query: '', gameId });
export const DEFAULT_LIBRARY = Object.freeze({ query: '', status: 'all', format: 'all', sort: 'name-asc' });
export function validColor(value) { return typeof value === 'string' && /^#[\da-f]{6}$/i.test(value); }
export function preferences(value = {}) {
  if (!record(value)) throw new Error('Invalid preview preferences.');
  return { themeId: clean(value.themeId, 'base', 64), accent: validColor(value.accent) ? value.accent : '#cd4451',
    reducedMotion: value.reducedMotion !== false, opaque: value.opaque !== false };
}
function boundedRecords(value, limit, label) {
  if (!Array.isArray(value) || value.length > limit || value.some(item => !record(item))) {
    throw new Error(`Invalid or oversized ${label} response.`);
  }
  return value;
}
export function normalizeSnapshot(value) {
  if (!record(value) || value.readOnly !== true || typeof value.sourceAttached !== 'boolean') {
    throw new Error('Native snapshot does not match the preview contract.');
  }
  const installations = boundedRecords(value.installations, 128, 'installation').map(item => ({
    id: clean(item.id, '', 80), name: clean(item.name, 'Installation'), gameId: clean(item.gameId),
    path: clean(item.path, '', 4096), current: item.current === true, selected: item.selected === true,
    available: typeof item.available === 'boolean' ? item.available : null
  }));
  if (installations.some(item => !/^(0|[1-9]\d{0,9})$/.test(item.id) || Number(item.id) > 0xffffffff)
    || new Set(installations.map(item => item.id)).size !== installations.length) {
    throw new Error('Ambiguous installation IDs in native snapshot.');
  }
  const games = boundedRecords(value.games, 256, 'game catalogue').map(item => ({
    id: clean(item.id, '', 120), name: clean(item.name, 'Game'), gamebanana: item.gamebanana === true
  }));
  if (games.some(item => !/^[a-z0-9][a-z0-9_.-]*$/i.test(item.id) || item.id.split('.').some(part => !part))
    || new Set(games.map(item => item.id)).size !== games.length) throw new Error('Invalid game catalogue IDs.');
  const selectedInstallationId = value.selectedInstallationId ?? null;
  if (selectedInstallationId !== null && !installations.some(item => item.id === selectedInstallationId && item.selected)
    || installations.filter(item => item.selected).length !== (selectedInstallationId === null ? 0 : 1)) {
    throw new Error('Native installation selection is inconsistent.');
  }
  const mods = boundedRecords(value.mods, 500, 'mod').map((item, i) => ({
    id: clean(item.id, String(i)), name: clean(item.name, 'Unnamed mod'),
    description: clean(item.description, '', 600), enabled: typeof item.enabled === 'boolean' ? item.enabled : null,
    format: clean(item.format, 'unknown', 40)
  }));
  if (!value.sourceAttached && (installations.length || mods.length || selectedInstallationId !== null)) {
    throw new Error('Detached snapshot still contains source-profile data.');
  }
  return {
    sourceAttached: value.sourceAttached, preferences: preferences(value.preferences),
    installations, selectedInstallationId, mods, games,
    themes: boundedRecords(value.themes, 128, 'theme').map(item => ({
      id: clean(item.id, 'base', 64), name: clean(item.name, 'Theme'),
      accent: validColor(item.accent) ? item.accent : '#cd4451'
    })),
    warnings: Array.isArray(value.warnings) ? value.warnings.slice(0, 12).map(item => clean(item, '', 512)) : [],
    readOnly: true
  };
}
export function normalizeShop(value) {
  if (!record(value)) throw new Error('Unexpected Mod Shop response.');
  return { items: boundedRecords(value.items, 24, 'Mod Shop').map(item => ({
    id: clean(item.id, '', 24), name: clean(item.name, 'Untitled mod'), author: clean(item.author),
    description: clean(item.description, '', 400), url: clean(item.url, '', 256)
  })).filter(item => /^https:\/\/gamebanana\.com\/mods\/[1-9]\d*$/.test(item.url)),
  hasMore: value.hasMore === true };
}
export function filterMods(mods, query, enabledOnly = false) {
  return queryMods(mods, { ...DEFAULT_LIBRARY, query, status: enabledOnly ? 'enabled' : 'all' });
}
export function queryMods(mods, options = DEFAULT_LIBRARY) {
  const term = String(options.query ?? '').trim().toLocaleLowerCase();
  const filtered = mods.filter(mod => {
    const status = mod.enabled === null ? 'unknown' : mod.enabled ? 'enabled' : 'disabled';
    return (options.status === 'all' || options.status === status)
      && (options.format === 'all' || options.format === mod.format)
      && `${mod.name} ${mod.description} ${mod.id}`.toLocaleLowerCase().includes(term);
  });
  // Sort a new array, never mutate the backend snapshot or its record order.
  return filtered.sort((a, b) => {
    if (options.sort === 'enabled-first') {
      const rank = value => value === true ? 0 : value === false ? 1 : 2;
      const delta = rank(a.enabled) - rank(b.enabled);
      if (delta) return delta;
    }
    const name = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
    return (options.sort === 'name-desc' ? -name : name) || a.id.localeCompare(b.id) || a.format.localeCompare(b.format);
  });
}
function defaultGame(snapshot) {
  const selected = snapshot.installations.find(item => item.id === snapshot.selectedInstallationId);
  // Never silently show a different game's mods for an unsupported selection.
  if (selected) return snapshot.games.find(game => game.id === selected.gameId && game.gamebanana)?.id || '';
  return snapshot.games.find(game => game.id === 'toby.deltarune' && game.gamebanana)?.id
    || snapshot.games.find(game => game.gamebanana)?.id || '';
}

/** UI state is independent of React/GPUIX. Source selection is never persisted. */
export class AppModel {
  constructor(bridge) {
    this.bridge = bridge;
    this.listeners = new Set();
    this.shortcutListeners = new Set();
    this.revision = 0;
    this.shopRevision = 0;
    this.profileChanging = false;
    this.disposed = false;
    this.state = { route: 'home', snapshot: null, error: '', loading: false, saving: false,
      profileEpoch: 0, library: { ...DEFAULT_LIBRARY }, history: { back: [], forward: [] }, shop: emptyShop() };
    this.subscribe = listener => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
    this.getSnapshot = () => this.state;
  }
  update(patch) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach(listener => listener());
  }
  navigate(route) {
    if (!ROUTES.includes(route) || route === this.state.route) return;
    this.update({ route, history: { back: [...this.state.history.back, this.state.route].slice(-32), forward: [] } });
  }
  goBack() {
    const { back, forward } = this.state.history;
    if (!back.length) return;
    this.update({ route: back.at(-1), history: { back: back.slice(0, -1), forward: [this.state.route, ...forward].slice(0, 32) } });
  }
  goForward() {
    const { back, forward } = this.state.history;
    if (!forward.length) return;
    this.update({ route: forward[0], history: { back: [...back, this.state.route].slice(-32), forward: forward.slice(1) } });
  }
  onShortcut(listener) { this.shortcutListeners.add(listener); return () => this.shortcutListeners.delete(listener); }
  dispatchShortcut(action) { if (!this.disposed) this.shortcutListeners.forEach(listener => listener(action)); }
  setLibrary(patch) {
    const next = { ...this.state.library, ...patch };
    if (!['all', 'enabled', 'disabled', 'unknown'].includes(next.status)
      || !['all', 'runtime', 'legacy packet'].includes(next.format)
      || !['name-asc', 'name-desc', 'enabled-first'].includes(next.sort)) return;
    next.query = clean(next.query, '', 128);
    this.update({ library: next });
  }
  async initialize() {
    const hello = await this.bridge.request('hello');
    const required = ['snapshot', 'profile.attach', 'profile.detach', 'installation.select', 'shop.browse', 'preferences.set'];
    if (!hello || hello.protocol !== 1 || hello.workspaceVersion !== 2 || hello.readOnly !== true
      || !Array.isArray(hello.capabilities) || required.some(item => !hello.capabilities.includes(item))) {
      throw new Error('The native backend is incompatible. Rebuild the GPUIX native host.');
    }
    return this.refresh();
  }
  acceptSnapshot(snapshot, changedProfile = false, resetLibrary = false) {
    const oldGame = this.state.shop.gameId;
    const selectionChanged = snapshot.selectedInstallationId !== this.state.snapshot?.selectedInstallationId;
    const gameId = !changedProfile && !selectionChanged && snapshot.games.some(game => game.id === oldGame && game.gamebanana)
      ? oldGame : defaultGame(snapshot);
    const resetShop = changedProfile || selectionChanged || gameId !== oldGame;
    if (resetShop) this.shopRevision++;
    this.update({ snapshot, loading: false, profileEpoch: this.state.profileEpoch + (changedProfile ? 1 : 0),
      ...(resetShop ? { shop: emptyShop(gameId) } : {}),
      ...(resetLibrary ? { library: { ...DEFAULT_LIBRARY } } : {}) });
  }
  async refresh() {
    if (this.disposed || this.profileChanging || this.state.saving) throw new Error('Preview is busy. Try again.');
    const revision = ++this.revision;
    this.update({ loading: true, error: '' });
    try {
      const value = normalizeSnapshot(await this.bridge.request('snapshot'));
      if (revision === this.revision && !this.disposed) this.acceptSnapshot(value);
      return value;
    } catch (error) {
      if (revision === this.revision) this.update({ error: message(error), loading: false });
      throw error;
    }
  }
  async changeProfile(command, args, resetLibrary) {
    if (this.disposed || this.profileChanging || this.state.loading || this.state.saving) return false;
    this.profileChanging = true;
    const revision = ++this.revision;
    this.shopRevision++;
    this.update({ loading: true, error: '', shop: emptyShop(this.state.shop.gameId) });
    try {
      const snapshot = normalizeSnapshot(await this.bridge.request(command, args));
      if (command === 'profile.detach' && snapshot.sourceAttached) throw new Error('Native profile was not detached.');
      if (command === 'profile.attach' && !snapshot.sourceAttached) throw new Error('Native profile was not attached.');
      if (command === 'installation.select' && snapshot.selectedInstallationId !== args.id) {
        throw new Error('Native installation selection was not acknowledged.');
      }
      if (revision !== this.revision || this.disposed) return false;
      this.profileChanging = false;
      this.acceptSnapshot(snapshot, true, resetLibrary);
      return true;
    } catch (error) {
      this.profileChanging = false;
      if (revision === this.revision) this.update({ error: message(error), loading: false });
      return false;
    } finally { this.profileChanging = false; }
  }
  attachProfile(path) { return this.changeProfile('profile.attach', { path }, true); }
  detachProfile() { return this.changeProfile('profile.detach', {}, true); }
  selectInstallation(id) { return this.changeProfile('installation.select', { id }, false); }
  async savePreferences(patch) {
    if (this.disposed || !this.state.snapshot || this.state.saving || this.state.loading) return;
    this.update({ saving: true, error: '' });
    try {
      const next = preferences(await this.bridge.request('preferences.set', { ...this.state.snapshot.preferences, ...patch }));
      this.update({ saving: false, snapshot: { ...this.state.snapshot, preferences: next } });
    } catch (error) { this.update({ saving: false, error: message(error) }); }
  }
  setShopGame(gameId) {
    if (this.profileChanging || !this.state.snapshot?.games.some(game => game.id === gameId && game.gamebanana)) return;
    if (gameId === this.state.shop.gameId) return;
    this.shopRevision++;
    this.update({ shop: emptyShop(gameId) });
  }
  async browse(query, page = 1) {
    if (this.disposed || this.profileChanging || this.state.loading) return;
    const gameId = this.state.shop.gameId;
    if (!this.state.snapshot?.games.some(game => game.id === gameId && game.gamebanana)) {
      this.update({ shop: { ...emptyShop(gameId), status: 'error', error: 'Choose a game with a GameBanana catalogue.' } });
      return;
    }
    query = clean(String(query).trim(), '', 128);
    page = Math.min(100, Math.max(1, Math.trunc(page) || 1));
    const revision = ++this.shopRevision;
    const epoch = this.state.profileEpoch;
    // Results from another query/page must not look like matches for this one.
    this.update({ shop: { ...emptyShop(gameId), status: 'loading', query, page } });
    try {
      const value = normalizeShop(await this.bridge.request('shop.browse', { query, page, gameId }));
      if (revision === this.shopRevision && epoch === this.state.profileEpoch) {
        this.update({ shop: { ...value, status: 'ready', error: '', query, page, gameId, hasMore: value.hasMore && page < 100 } });
      }
    } catch (error) {
      if (revision === this.shopRevision && epoch === this.state.profileEpoch) {
        this.update({ shop: { ...this.state.shop, status: 'error', error: message(error), items: [], hasMore: false } });
      }
    }
  }
  dispose() {
    this.disposed = true; this.revision++; this.shopRevision++;
    this.listeners.clear(); this.shortcutListeners.clear(); this.bridge.close();
  }
}

/** Native GPUIX key names/modifiers, not browser KeyboardEvent properties. */
export function shortcutFor(event, platform = process.platform) {
  if (!event || typeof event.key !== 'string' || event.isHeld) return null;
  const key = event.key.toLowerCase();
  const modifiers = event.modifiers || {};
  const primary = platform === 'darwin' ? modifiers.cmd : modifiers.ctrl;
  const secondary = platform === 'darwin' ? modifiers.ctrl : modifiers.cmd;
  if (secondary) return null;
  if (key === 'tab' && !primary && !modifiers.alt) return { action: modifiers.shift ? 'tab-previous' : 'tab-next' };
  if (key === 'escape' && !primary && !modifiers.alt && !modifiers.shift) return { action: 'close' };
  if (key === 'f1' && !primary && !modifiers.alt && !modifiers.shift) return { action: 'help' };
  if (modifiers.alt && !primary && !modifiers.shift) {
    if (key === 'left') return { action: 'back' };
    if (key === 'right') return { action: 'forward' };
  }
  // AltGr and mixed modifier chords must not trigger application navigation.
  if (!primary || modifiers.alt) return null;
  if (modifiers.shift) return key === 'o' ? { action: 'attach' } : null;
  if (/^[1-6]$/.test(key)) return { action: 'navigate', route: ROUTES[Number(key) - 1] };
  if (key === 'r') return { action: 'refresh' };
  if (key === 'f') return { action: 'search' };
  return null;
}
