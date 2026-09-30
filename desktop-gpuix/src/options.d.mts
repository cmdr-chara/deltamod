// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { spawn } from 'node:child_process';
export interface Options {
  resourcesRoot: string;
  stateRoot: string;
  sourceProfile: string;
  managedDataRoot: string;
  executable: string;
  focus: boolean;
  openLink: string;
  benchmarkFile: string;
  reducedMotion: boolean | null;
  opaque: boolean | null;
}
export function parseOptions(argv: string[], env?: NodeJS.ProcessEnv, platform?: NodeJS.Platform): Options;
export function openModPage(raw: string, spawnImpl?: typeof spawn, platform?: NodeJS.Platform): Promise<void>;
