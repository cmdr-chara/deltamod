// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export type Route = 'home' | 'library' | 'installations' | 'shop' | 'themes' | 'settings';
export interface Preferences { themeId: string; accent: string; reducedMotion: boolean; opaque: boolean }
export interface Installation { id: string; name: string; gameId: string; path: string; current: boolean; selected: boolean; available: boolean | null }
export interface Mod { id: string; name: string; description: string; enabled: boolean | null; format: string }
export interface Theme { id: string; name: string; accent: string }
export interface Game { id: string; name: string; gamebanana: boolean }
export interface LibraryQuery { query: string; status: 'all' | 'enabled' | 'disabled' | 'unknown'; format: 'all' | 'runtime' | 'legacy packet'; sort: 'name-asc' | 'name-desc' | 'enabled-first' }
export type Shortcut = { action: 'navigate'; route: Route } | { action: 'back' | 'forward' | 'refresh' | 'search' | 'attach' | 'help' | 'close' | 'tab-next' | 'tab-previous' };
export interface Snapshot { selectedInstallationId: string | null; games: Game[]; sourceAttached: boolean; readOnly: true; preferences: Preferences; installations: Installation[]; mods: Mod[]; themes: Theme[]; warnings: string[] }
export interface ShopItem { id: string; name: string; author: string; description: string; url: string }
export interface Shop { gameId: string; status: 'idle' | 'loading' | 'ready' | 'error'; items: ShopItem[]; error: string; hasMore: boolean; page: number; query: string }
export interface State { profileEpoch: number; library: LibraryQuery; history: { back: Route[]; forward: Route[] }; route: Route; snapshot: Snapshot | null; error: string; loading: boolean; saving: boolean; shop: Shop }
export interface BackendClient { request(command: string, args?: Record<string, unknown>): Promise<unknown>; close(): void }
