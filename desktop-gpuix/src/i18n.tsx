// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createContext, useContext, useMemo } from 'react';
import { translate, formatBytes } from './locales.mjs';
export const LanguageContext = createContext<'en' | 'it'>('en');
export function useMessages() {
  const locale = useContext(LanguageContext);
  return useMemo(() => (key: string, values?: Record<string, string | number>) => translate(locale, key, values), [locale]);
}
export function useByteFormat() {
  const locale = useContext(LanguageContext);
  return useMemo(() => (bytes: number | null) => formatBytes(locale, bytes), [locale]);
}
