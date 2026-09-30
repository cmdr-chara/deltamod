// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const ROUTES = new Set(['home', 'library', 'installations', 'shop', 'themes', 'settings']);
export const PREVIEW_SCHEME = 'deltamod-gpuix-preview:';
export function validModId(value) {
  return typeof value === 'string' && /^[1-9]\d{0,15}$/.test(value)
    && Number.isSafeInteger(Number(value));
}
export function validGameId(value) {
  return typeof value === 'string' && value.length <= 120 && /^[a-z0-9][a-z0-9_.-]*$/i.test(value)
    && value.split('.').every(Boolean);
}

/** Parse a read-only navigation intent. Never registers a protocol, opens a
 * browser, attaches a filesystem path, downloads, installs or starts a game. */
export function parseIntent(raw) {
  if (typeof raw !== 'string' || raw.length > 2048 || !raw.length || /[\u0000-\u0020\u007f\\]/.test(raw)
    || /%(?![0-9a-f]{2})/i.test(raw)) throw new Error('links.invalid');
  let url;
  try { url = new URL(raw); } catch { throw new Error('links.invalid'); }
  if (url.username || url.password || url.port || url.hash) throw new Error('links.invalid');
  if (url.protocol === 'https:') {
    const match = /^https:\/\/gamebanana\.com\/mods\/([1-9]\d{0,15})$/.exec(raw);
    if (!match || !validModId(match[1])) throw new Error('links.invalid');
    return Object.freeze({ kind: 'mod', id: match[1] });
  }
  if (url.protocol !== PREVIEW_SCHEME || (url.pathname !== '' && url.pathname !== '/')) throw new Error('links.invalid');
  const parameters = Object.create(null);
  for (const [key, value] of url.searchParams) {
    if (Object.hasOwn(parameters, key) || /[\u0000-\u001f\u007f]/.test(value)) throw new Error('links.invalid');
    parameters[key] = value;
  }
  const only = (...keys) => Object.keys(parameters).every(key => keys.includes(key));
  switch (url.hostname) {
    case 'screen':
      if (!only('route') || !ROUTES.has(parameters.route)) break;
      return Object.freeze({ kind: 'screen', route: parameters.route });
    case 'browse':
      if (!only('game', 'q') || !validGameId(parameters.game) || [...(parameters.q || '')].length > 128) break;
      return Object.freeze({ kind: 'browse', gameId: parameters.game, query: (parameters.q || '').trim() });
    case 'mod':
      if (!only('id') || !validModId(parameters.id)) break;
      return Object.freeze({ kind: 'mod', id: parameters.id });
  }
  throw new Error('links.invalid');
}
