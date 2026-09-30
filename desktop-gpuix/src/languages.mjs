// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const languages = Object.freeze([
  { id: 'en', label: 'English' }, { id: 'it', label: 'Italiano' },
  { id: 'de', label: 'Deutsch' }, { id: 'es', label: 'Español' },
  { id: 'fr', label: 'Français' }, { id: 'ja', label: '日本語' },
  { id: 'pl', label: 'Polski' }, { id: 'pt-br', label: 'Português (Brasil)' },
].map(Object.freeze));
export function supportedLocale(value) { return languages.some(language => language.id === value); }
