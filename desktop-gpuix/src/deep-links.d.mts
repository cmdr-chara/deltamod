// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { Route } from './contracts.js';
export const PREVIEW_SCHEME: 'deltamod-gpuix-preview:';
export type LinkIntent = Readonly<{ kind: 'screen'; route: Route } | { kind: 'browse'; gameId: string; query: string } | { kind: 'mod'; id: string }>;
export function validModId(value: unknown): value is string;
export function validGameId(value: unknown): value is string;
export function parseIntent(raw: string): LinkIntent;
