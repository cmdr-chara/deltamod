// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useSyncExternalStore } from 'react';
import type { UpdateRuntime } from './updater.mjs';
import { Action, GlassPanel, Label, column, row } from './ui.js';

export function UpdatePanel({ runtime }: { runtime: UpdateRuntime }) {
  const state = useSyncExternalStore(runtime.subscribe, runtime.getSnapshot);
  const busy = state.status === 'checking' || state.status === 'installing';
  return <GlassPanel testId="gpuix-updater">
    <div style={{ ...column, gap: 5 }}><Label size={18} bold>GPUIX updates</Label>
      <Label muted>Signed packages use a separate GPUIX feed and cannot consume the Tauri updater channel.</Label></div>
    <div style={row}><Action disabled={busy} onClick={() => void runtime.check()}>{state.status === 'checking' ? 'Checking...' : 'Check for updates'}</Action>
      {state.status === 'available' && state.update && <Action primary disabled={busy} onClick={() => void runtime.install()}>Install {state.update.version}</Action>}</div>
    {state.status === 'current' && <Label muted>You are running the latest GPUIX package.</Label>}
    {state.status === 'installed' && <Label>Update verified and installed. Quit and reopen Deltamod to use it.</Label>}
    {state.status === 'error' && <Label>{state.error}</Label>}
  </GlassPanel>;
}
