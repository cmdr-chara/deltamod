// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2

/** Coalesces authenticated handoffs arriving before the native window mounts.
 * Queue acceptance and OS foreground permission are deliberately independent.
 * There are no timers, synthetic clicks, or process-name based activation. */
export class WindowActivation {
  constructor(report = () => {}) {
    this.activate = null;
    this.pending = false;
    this.disposed = false;
    this.report = report;
  }
  request() {
    if (this.disposed) return;
    this.pending = true;
    this.flush();
  }
  bind(activate) {
    if (typeof activate !== 'function') throw new TypeError('A native activation callback is required.');
    if (this.disposed) return () => {};
    this.activate = activate;
    this.flush();
    return () => { if (this.activate === activate) this.activate = null; };
  }
  flush() {
    if (!this.pending || !this.activate || this.disposed) return;
    this.pending = false;
    try { this.activate(); }
    catch { this.report('The open request was queued, but the operating system could not foreground the window.'); }
  }
  dispose() { this.disposed = true; this.pending = false; this.activate = null; }
}

/** Restore focus only to a still-mounted native element. The dialog owner
 * checks modal nesting before invoking this helper. */
export function restoreDialogFocus(renderer, previousId, popupId) {
  if (!Number.isSafeInteger(previousId) || previousId < 0 || previousId === popupId) return false;
  if (typeof renderer.getElementBounds !== 'function' || typeof renderer.focusElement !== 'function') return false;
  try {
    if (!renderer.getElementBounds(previousId)) return false;
    renderer.focusElement(previousId);
    return true;
  } catch { return false; }
}
