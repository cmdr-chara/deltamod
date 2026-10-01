// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const LAUNCH_MARKER: string;
export interface LaunchMarker { readonly path: string }
export function launchMarkerPath(raw: unknown, platform?: string): string;
export function readLaunchMarker(raw: unknown, tempRoot?: string): LaunchMarker;
export function acknowledgeLaunchMarker(ticket: LaunchMarker): boolean;
export function abandonLaunchMarker(ticket: LaunchMarker): void;
