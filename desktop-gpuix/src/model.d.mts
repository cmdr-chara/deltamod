// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { BackendClient, State, Snapshot, Preferences, Mod, ShopItem, Route, LibraryQuery, Shortcut } from './contracts.js';
export const DEFAULT_LIBRARY: Readonly<LibraryQuery>;
export function queryMods(mods: Mod[], options?: LibraryQuery): Mod[];
export const ROUTES: readonly Route[];
export function validColor(value: unknown): value is string;
export function preferences(value?: unknown): Preferences;
export function normalizeSnapshot(value: unknown): Snapshot;
export function normalizeShop(value: unknown): { items: ShopItem[]; hasMore: boolean };
export function filterMods(mods: Mod[], query: string, enabledOnly?: boolean): Mod[];
export class AppModel {
  constructor(bridge: BackendClient);
  state: State;
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => State;
  update(patch: Partial<State>): void;
  initialize(): Promise<Snapshot>;
  refresh(): Promise<Snapshot>;
  navigate(route: Route): void;
  attachProfile(path: string): Promise<boolean>;
  detachProfile(): Promise<boolean>;
  selectInstallation(id: string): Promise<boolean>;
  setShopGame(gameId: string): void;
  setLibrary(patch: Partial<LibraryQuery>): void;
  goBack(): void;
  goForward(): void;
  onShortcut(listener: (action: Shortcut) => void): () => void;
  dispatchShortcut(action: Shortcut): void;
  savePreferences(patch: Partial<Preferences>): Promise<void>;
  browse(query: string, page?: number): Promise<void>;
  dispose(): void;
}

export function shortcutFor(event: { key?: string; isHeld?: boolean; modifiers?: { shift?: boolean; ctrl?: boolean; alt?: boolean; cmd?: boolean } }, platform?: string): Shortcut | null;
