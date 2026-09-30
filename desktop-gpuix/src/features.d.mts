// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { BackendClient } from './contracts.js';
import type { AppModel } from './model.mjs';
import type { LinkIntent } from './deep-links.mjs';
export interface InterfacePreferences { schemaVersion: 1; locale: 'en' | 'it'; themeImages: boolean }
export interface ModDetail { id: string; name: string; author: string; game: string; description: string; url: string; hasContentRatings: boolean; files: { id: string; name: string; version: string; bytes: number | null }[] }
export interface ThemePreview { themeId: string; imagePath: string | null; hasVideo: boolean; hasAudio: boolean }
export interface FeatureLoad<T> { status: 'idle' | 'loading' | 'ready' | 'error'; id: string; value: T | null; error: string }
export interface FeatureState { preferences: InterfacePreferences; saving: boolean; error: string; detail: FeatureLoad<ModDetail>; theme: FeatureLoad<ThemePreview>; pendingLink: LinkIntent | null }
export function interfacePreferences(value: unknown): InterfacePreferences;
export function plainDescription(value: unknown): string;
export function normalizeDetail(value: unknown, requestedId: string): ModDetail;
export function normalizeTheme(value: unknown, requestedId: string): ThemePreview;
export class DesktopFeatures {
  constructor(model: AppModel, bridge: BackendClient);
  state: FeatureState;
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => FeatureState;
  initialize(): Promise<InterfacePreferences>;
  update(patch: Partial<FeatureState>): void;
  savePreferences(patch: Partial<InterfacePreferences>): Promise<boolean>;
  openDetail(id: string): Promise<boolean>;
  closeDetail(): void;
  loadTheme(id: string): Promise<boolean>;
  retryTheme(): Promise<boolean>;
  reviewLink(raw: string): boolean;
  dismissLink(): void;
  applyLink(): boolean;
  dispose(): void;
}
