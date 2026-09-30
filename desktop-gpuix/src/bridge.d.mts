// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
export const PROTOCOL: 1;
export const MAX_REQUEST_BYTES: number;
export const MAX_RESPONSE_BYTES: number;
export class Bridge {
  child: ChildProcessWithoutNullStreams;
  closed: boolean;
  constructor(child: ChildProcessWithoutNullStreams, options?: { timeoutMs?: number; maxPending?: number });
  request(command: string, args?: Record<string, unknown>): Promise<unknown>;
  close(error?: Error): void;
}
export function startBridge(options: { executable: string; stateRoot: string; resourcesRoot: string; sourceProfile?: string; managedDataRoot?: string }): Bridge;
