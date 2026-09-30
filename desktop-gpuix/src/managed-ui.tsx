// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useMemo, useState, useSyncExternalStore } from 'react';
import { useGpuixRequired } from '@gpuix/react';
import type { ManagedMod, ManagedRuntime } from './managed.mjs';
import { Action, Chip, Empty, GlassPanel, Label, column, row } from './ui.js';

export function ManagedLibraryPanel({ runtime }: { runtime: ManagedRuntime }) {
  const state = useSyncExternalStore(runtime.subscribe, runtime.getSnapshot);
  const renderer = useGpuixRequired();
  const [selected, setSelected] = useState<string[]>([]);
  const [pendingRemove, setPendingRemove] = useState('');
  const busy = !!state.busy || state.loading;
  const mods = state.catalog?.mods ?? [];
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const recoveries = useMemo(() => {
    const ids = new Set<string>();
    for (const journal of state.catalog?.journals ?? []) {
      const phase = typeof journal.phase === 'string' ? journal.phase : '';
      const installation = typeof journal.installationId === 'string' ? journal.installationId : '';
      if (installation && phase && phase !== 'complete') ids.add(installation);
    }
    return [...ids];
  }, [state.catalog]);

  async function chooseArchive(replaceExisting: boolean) {
    if (busy || !renderer.promptForPaths) return;
    try {
      const files = await renderer.promptForPaths({
        files: true, directories: false, multiple: false,
        prompt: replaceExisting ? 'Replace mod from archive' : 'Import mod archive',
      });
      if (files?.[0]) await runtime.importArchive(files[0], replaceExisting);
    } catch {
      // Picker cancellation and picker errors are non-mutating.
    }
  }
  function toggleSelection(mod: ManagedMod) {
    setSelected(value => value.includes(mod.instanceId)
      ? value.filter(id => id !== mod.instanceId)
      : [...value, mod.instanceId].slice(0, 1000));
  }

  return <GlassPanel testId="managed-library" style={{ gap: 14 }}>
    <div style={{ ...row, justifyContent: 'space-between' }}>
      <div style={column}><Label size={19} bold>Managed library</Label>
        <Label muted size={12}>Transactional operations use the same Rust lifecycle and recovery store as Tauri.</Label></div>
      <div style={row}><Action disabled={busy} onClick={() => void runtime.refresh()}>Refresh managed</Action>
        <Action disabled={busy} onClick={() => void chooseArchive(false)}>Import archive</Action>
        <Action disabled={busy} onClick={() => void chooseArchive(true)}>Replace from archive</Action></div>
    </div>
    {state.error && <Label>{state.error}</Label>}
    {state.loading && <Label muted>Loading managed lifecycle state...</Label>}
    {!state.loading && mods.length === 0 && <Empty title="No managed mods">Import a local Deltamod-compatible archive, or point the preview at an existing managed data root.</Empty>}
    {mods.length > 0 && <virtual-list estimatedItemHeight={118} style={{ height: 330 }}>
      {mods.map(mod => {
        const enabled = state.enabledIds.includes(mod.instanceId);
        const runtimeMod = mod.installationId === 'local-mod-library';
        const removing = pendingRemove === mod.instanceId;
        return <div key={mod.installationId + ':' + mod.instanceId} style={{ paddingBottom: 9 }}>
          <GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 13 }}>
            <div style={{ ...column, gap: 5, minWidth: 0, flexGrow: 1 }}>
              <Label bold>{mod.name}</Label>
              <div style={row}><Chip active={selectedSet.has(mod.instanceId)}>{selectedSet.has(mod.instanceId) ? 'Patch selected' : mod.installationId}</Chip>
                {mod.version && <Label muted size={12}>{mod.version}</Label>}<Label muted size={12}>{mod.files} files</Label></div>
            </div>
            <div style={{ ...row, justifyContent: 'flex-end' }}>
              <Action disabled={busy} onClick={() => toggleSelection(mod)}>{selectedSet.has(mod.instanceId) ? 'Unselect' : 'Select'}</Action>
              {runtimeMod && <Action disabled={busy} onClick={() => void runtime.toggle(mod.instanceId, !enabled)}>{enabled ? 'Disable' : 'Enable'}</Action>}
              <Action disabled={busy} onClick={() => void runtime.verify(mod)}>Verify</Action>
              <Action disabled={busy} onClick={() => void runtime.repair(mod)}>Repair</Action>
              {!removing ? <Action disabled={busy} onClick={() => setPendingRemove(mod.instanceId)}>Remove</Action>
                : <><Action disabled={busy} onClick={() => { setPendingRemove(''); void runtime.uninstall(mod); }}>Confirm remove</Action>
                  <Action disabled={busy} onClick={() => setPendingRemove('')}>Cancel</Action></>}
            </div>
          </GlassPanel>
        </div>;
      })}
    </virtual-list>}
    <div style={{ ...row }}>
      <Action primary disabled={busy || selected.length === 0} onClick={() => void runtime.patch(selected)}>Patch & run selected</Action>
      <Action disabled={state.busy !== 'managed.patch.run'} onClick={() => void runtime.cancelPatch()}>Cancel patch</Action>
      <Action disabled={busy} onClick={() => void runtime.hashes()}>Recalculate game hashes</Action>
      <Action disabled={busy} onClick={() => void runtime.launch()}>Launch current game</Action>
    </div>
    {recoveries.map(id => <div key={id} style={{ ...row, justifyContent: 'space-between' }}>
      <Label>Recovery is available for {id}</Label><Action disabled={busy} onClick={() => void runtime.restore(id)}>Restore last working state</Action>
    </div>)}
    {state.lastOperation != null && <Label muted size={12}>Last operation acknowledged by native runtime.</Label>}
  </GlassPanel>;
}

export function ManagedSystemPanel({ runtime }: { runtime: ManagedRuntime }) {
  const state = useSyncExternalStore(runtime.subscribe, runtime.getSnapshot);
  const busy = !!state.busy || state.loading;
  const present = state.credentials && typeof state.credentials.present === 'object' && state.credentials.present
    ? state.credentials.present as Record<string, boolean> : {};
  return <GlassPanel testId="managed-system" style={{ gap: 12 }}>
    <Label size={18} bold>Native runtime</Label>
    <Label muted>Game launch, lifecycle journals, patching and secure credentials run without a Tauri window or WebView.</Label>
    <div style={{ ...row }}>
      <Action disabled={busy} onClick={() => void runtime.refresh()}>Refresh runtime</Action>
      <Action disabled={busy} onClick={() => void runtime.launch()}>Launch current game</Action>
    </div>
    <Label muted size={12}>Secure credential store: GameBanana {present['gamebanana-cookies'] ? 'present' : 'not present'} · Nexus {present['nexus-oauth-tokens'] ? 'present' : 'not present'}</Label>
    <div style={{ ...row }}>
      <Action disabled={busy || !present['gamebanana-cookies']} onClick={() => void runtime.clearCredential('gamebanana')}>Clear GameBanana login</Action>
      <Action disabled={busy || !present['nexus-oauth-tokens']} onClick={() => void runtime.clearCredential('nexus')}>Clear Nexus login</Action>
    </div>
    <Label muted size={12}>New interactive provider login still requires a provider-safe authentication surface. Existing secrets use the same OS keyring service.</Label>
    {state.error && <Label>{state.error}</Label>}
  </GlassPanel>;
}
