// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export function createLauncherReady(env?: NodeJS.ProcessEnv, output?: { write(value: string): unknown }): (options: { stateRoot: string; managedDataRoot?: string }) => void;
