// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export class WindowActivation {
  constructor(report?: (message: string) => void);
  request(): void;
  bind(activate: () => void): () => void;
  dispose(): void;
}
export function restoreDialogFocus(renderer: {
  getElementBounds?: (id: number) => unknown;
  focusElement?: (id: number) => void;
}, previousId: number | null | undefined, popupId?: number): boolean;
