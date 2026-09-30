// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { BackendClient, State, Snapshot, Preferences, Mod, ShopItem, Route } from './contracts.js';
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
  attachProfile(path: string): Promise<void>;
  savePreferences(patch: Partial<Preferences>): Promise<void>;
  browse(query: string, page?: number): Promise<void>;
  dispose(): void;
}
