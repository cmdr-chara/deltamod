// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export type Route = 'home' | 'library' | 'installations' | 'shop' | 'themes' | 'settings';
export interface Preferences { themeId: string; accent: string; reducedMotion: boolean; opaque: boolean }
export interface Installation { id: string; name: string; gameId: string; path: string; current: boolean; available: boolean | null }
export interface Mod { id: string; name: string; description: string; enabled: boolean | null; format: string }
export interface Theme { id: string; name: string; accent: string }
export interface Snapshot { sourceAttached: boolean; readOnly: true; preferences: Preferences; installations: Installation[]; mods: Mod[]; themes: Theme[]; warnings: string[] }
export interface ShopItem { id: string; name: string; author: string; description: string; url: string }
export interface Shop { status: 'idle' | 'loading' | 'ready' | 'error'; items: ShopItem[]; error: string; hasMore: boolean; page: number; query: string }
export interface State { route: Route; snapshot: Snapshot | null; error: string; loading: boolean; saving: boolean; shop: Shop }
export interface BackendClient { request(command: string, args?: Record<string, unknown>): Promise<unknown>; close(): void }
