// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export type Locale = 'en'|'it'|'de'|'es'|'fr'|'ja'|'pl'|'pt-br';
export const languages:readonly Readonly<{id:Locale;label:string}>[];
export function supportedLocale(value:unknown):value is Locale;
