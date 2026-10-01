// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { spawn } from 'node:child_process';
import type { PlaybackCoordinator, PlaybackOwner } from './media-owner.mjs';
export const VIDEO: Readonly<{ width: number; height: number; fps: number; bytes: number }>;
export interface ThemeMedia { themeId: string; video: string | null; audio: string | null; cue: number | null; soulColor: string }
export function loadThemeMedia(root: string, themeId: string): ThemeMedia;
export function mediaTools(root: string): { ffmpeg: string; ffplay: string };
export interface PlaybackOptions { muted?: boolean; volume?: number; reducedMotion?: boolean; fromCue?: boolean; repeat?: boolean }
export function playbackPlan(theme: ThemeMedia, options?: PlaybackOptions): { offset: number; video: string[] | null; audio: string[] | null };
export class FrameAssembler { constructor(deliver: (pixels: Buffer) => void); push(chunk: Buffer): void; }
export class NativeThemePlayer implements PlaybackOwner {
  readonly disposed: boolean;
  readonly blocked: boolean;
  readonly children: ReadonlySet<unknown>;
  constructor(theme: ThemeMedia, tools: { ffmpeg: string; ffplay: string }, callbacks?: { onFrame?: (pixels: Buffer) => void; onState?: (state: string, error: string) => void; onTime?: (seconds: number, cue: boolean) => void; spawnImpl?: typeof spawn; coordinator?: PlaybackCoordinator });
  play(options?: PlaybackOptions): Promise<boolean>;
  stop(): Promise<void>;
  dispose(): void;
}
