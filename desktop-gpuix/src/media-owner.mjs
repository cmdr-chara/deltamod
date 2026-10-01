// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2

/** Process ownership spans player instances, not just repeated Play calls on
 * one widget. A theme remount must wait for the previous decoder to close. */
export class PlaybackCoordinator {
  constructor() { this.current = null; this.generation = 0; this.tail = Promise.resolve(); }
  claim(owner, start) {
    const generation = ++this.generation;
    const result = this.tail.then(async () => {
      if (generation !== this.generation || owner.disposed) return false;
      const previous = this.current;
      if (previous && previous !== owner) {
        await previous.stop();
        if (previous.blocked || previous.children.size) throw new Error('A previous native player could not be stopped. Restart the application.');
      }
      if (generation !== this.generation || owner.disposed) return false;
      this.current = owner;
      return start();
    });
    // A failed replacement must not poison the serialization chain. The
    // previous owner remains authoritative until its processes really close.
    this.tail = result.catch(() => false);
    return result;
  }
}

export const themePlaybackCoordinator = new PlaybackCoordinator();
