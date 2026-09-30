// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const dictionaries: Readonly<Record<'en' | 'it', Readonly<Record<string, string>>>>;
export function normalizeLocale(value: unknown): 'en' | 'it';
export function placeholders(template: string): string[];
export function translate(locale: unknown, key: string, values?: Record<string, string | number>): string;
export function formatBytes(locale: unknown, bytes: number | null): string;
