// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useLayoutEffect } from 'react';
import { useGpuixRequired } from '@gpuix/react';
import type { WindowActivation } from './window-lifecycle.mjs';

export function NativeWindow({ activation }: { activation: WindowActivation }) {
  const renderer = useGpuixRequired();
  useLayoutEffect(() => activation.bind(() => {
    if (!renderer.activateWindow) throw new Error('Native window activation is unavailable.');
    renderer.activateWindow();
  }), [renderer, activation]);
  return null;
}
