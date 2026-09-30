// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { translate as existingTranslate } from './locales.mjs';
import { sourceDictionaries } from './generated/source-locales.mjs';
import { supportedLocale } from './languages.mjs';
export function translate(locale, key, values = {}) {
  // Keep the existing reviewed English/Italian catalogue authoritative.
  if (locale === 'en' || locale === 'it' || !supportedLocale(locale)) return existingTranslate(locale, key, values);
  const english = existingTranslate('en', key);
  const catalogue = sourceDictionaries[locale];
  const template = catalogue && Object.hasOwn(catalogue, english) ? catalogue[english] : english;
  return template.replace(/\{([a-zA-Z][a-zA-Z0-9_]*)\}/g, (match, name) =>
    Object.hasOwn(values, name) && ['string', 'number'].includes(typeof values[name]) ? String(values[name]) : match);
}
const formats = new Map();
export function formatBytes(locale, bytes) {
  if (!Number.isSafeInteger(bytes) || bytes < 0) return translate(locale, 'Unknown size');
  const language = supportedLocale(locale) ? locale : 'en';
  if (!formats.has(language)) formats.set(language, new Intl.NumberFormat(language, { maximumFractionDigits: 2 }));
  const unit = bytes === 0 ? 0 : Math.min(4, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${formats.get(language).format(bytes / 1024 ** unit)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][unit]}`;
}
