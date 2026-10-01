// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export interface PlaybackOwner {
  readonly disposed: boolean;
  readonly blocked: boolean;
  readonly children: ReadonlySet<unknown>;
  stop(): Promise<void>;
}
export class PlaybackCoordinator {
  claim(owner: PlaybackOwner, start: () => Promise<boolean>): Promise<boolean>;
}
export const themePlaybackCoordinator: PlaybackCoordinator;
