// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
// The inherited language files are JSON with comments, not executable JavaScript.
export function parseCatalogue(source) {
  if (typeof source !== 'string' || Buffer.byteLength(source) > 256 * 1024) throw new Error('Language catalogue exceeds the size limit.');
  let clean = '', quoted = false;
  for (let i = 0; i < source.length; i++) {
    const char = source[i], next = source[i + 1];
    if (quoted) {
      clean += char;
      if (char === '\\') clean += source[++i] ?? '';
      else if (char === '"') quoted = false;
    } else if (char === '"') { quoted = true; clean += char; }
    else if (char === '/' && next === '/') {
      while (i + 1 < source.length && source[i + 1] !== '\n') i++;
      clean += ' ';
    } else if (char === '/' && next === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end < 0) throw new Error('Unclosed language catalogue comment.');
      clean += ' '; i = end + 1;
    } else clean += char;
  }
  // Remove JSONC trailing commas only outside strings.
  let json = ''; quoted = false;
  for (let i = 0; i < clean.length; i++) {
    const char = clean[i];
    if (quoted) {
      json += char;
      if (char === '\\') json += clean[++i] ?? '';
      else if (char === '"') quoted = false;
    } else if (char === '"') { quoted = true; json += char; }
    else if (char !== ',' || !/^\s*[}\]]/.test(clean.slice(i + 1))) json += char;
  }
  const value = JSON.parse(json.replace(/^\uFEFF/, ''));
  if (value === null || typeof value !== 'object' || Array.isArray(value)
    || Object.keys(value).length > 4000) throw new Error('Invalid language catalogue.');
  return value;
}
const plain = value => typeof value === 'string' && value.length > 0 && value.length <= 8192
  && !/[<>\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(value);
const tokens = value => [...value.matchAll(/\{([^{}]+)\}/g)].map(match => match[1]).sort().join('\0');
export function reuseCatalogue(english, translated) {
  const candidates = new Map();
  const ambiguous = new Set();
  for (const [key, source] of Object.entries(english)) {
    const target = Object.hasOwn(translated, key) ? translated[key] : null;
    if (!plain(source) || !plain(target) || tokens(source) !== tokens(target)) continue;
    // Exact English-text matching only. Do not infer synonyms, alter source
    // punctuation, rewrite placeholders, or transfer executable/HTML strings.
    if (candidates.has(source) && candidates.get(source) !== target) ambiguous.add(source);
    else candidates.set(source, target);
  }
  return Object.fromEntries([...candidates].filter(([source]) => !ambiguous.has(source)
    && !['__proto__', 'constructor', 'prototype'].includes(source)).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0));
}
