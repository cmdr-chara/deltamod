// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const dialogs = new Set();
const listeners = new Set();
export const subscribeDialogs = listener => { listeners.add(listener); return () => listeners.delete(listener); };
export const dialogCount = () => dialogs.size;
export function registerDialog() {
  const dialog = Symbol('dialog');
  dialogs.add(dialog);
  listeners.forEach(listener => listener());
  return () => { if (dialogs.delete(dialog)) listeners.forEach(listener => listener()); };
}
