// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useState, useSyncExternalStore, useEffect, useMemo } from 'react';
import { motion, useGpuixRequired, useWindowSize } from '@gpuix/react';
import type { AppModel } from './model.mjs';
import { filterMods } from './model.mjs';
import { openModPage } from './options.mjs';
import type { Mod, Installation, Preferences, Route, Snapshot } from './contracts.js';
import { Palette, colors, row, column, Label, Action, GlassPanel, Page, Empty, Chip, InfoDialog } from './ui.js';

type Modal = { kind: 'mod'; item: Mod } | { kind: 'installation'; item: Installation } | { kind: 'about' } | null;
const routes: { id: Route; label: string; icon: string }[] = [
  { id: 'home', label: 'Home', icon: 'home' }, { id: 'library', label: 'Mod library', icon: 'library' },
  { id: 'installations', label: 'Installations', icon: 'folder' }, { id: 'shop', label: 'Mod Shop', icon: 'shop' },
  { id: 'themes', label: 'Themes', icon: 'heart' }, { id: 'settings', label: 'Settings', icon: 'settings' },
];
const paths: Record<string, string> = {
  home: 'M3 11 12 3l9 8v10h-6v-7H9v7H3z', library: 'M4 3h4v18H4zM11 3h4v18h-4zM18 4l3 16',
  folder: 'M3 6h7l2 3h9v11H3z', shop: 'M3 9l3-5h12l3 5M4 9v12h16V9M9 21v-7h6v7',
  heart: 'M12 21 3 12C-3 3 8-1 12 6c4-7 15-3 9 6z', settings: 'M12 3v4m0 10v4M3 12h4m10 0h4M5 5l3 3m8 8 3 3M5 19l3-3m8-8 3-3M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0',
};
function Icon({ name, color = colors.muted, size = 20 }: { name: string; color?: string; size?: number }) {
  return <svg source={`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="${paths[name] || paths.home}" fill="none" stroke="#000" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`}
    style={{ width: size, height: size, color }} />;
}

export function App({ model, overrides, onCommitted }: { model: AppModel; overrides: { reducedMotion: boolean | null; opaque: boolean | null }; onCommitted: () => void }) {
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const renderer = useGpuixRequired();
  const size = useWindowSize();
  const [collapsed, setCollapsed] = useState(false);
  const [modal, setModal] = useState<Modal>(null);
  const snapshot = state.snapshot;
  const prefs = snapshot?.preferences || { themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true };
  const theme: Preferences = { ...prefs, reducedMotion: overrides.reducedMotion ?? prefs.reducedMotion, opaque: overrides.opaque ?? prefs.opaque };
  const compact = collapsed || size.width < 1000;
  useEffect(() => { if (snapshot) onCommitted(); }, [snapshot, onCommitted]);
  useEffect(() => { if (state.route === 'shop' && state.shop.status === 'idle') void model.browse(''); }, [state.route, state.shop.status, model]);

  async function attach() {
    try {
      if (!renderer.promptForPaths) throw new Error('Native folder picker is unavailable on this runtime.');
      const files = await renderer.promptForPaths({ files: false, directories: true, multiple: false, prompt: 'Read a Deltamod profile' });
      if (files?.[0]) await model.attachProfile(files[0]);
    } catch (error) { model.update({ error: error instanceof Error ? error.message : String(error) }); }
  }
  return <Palette.Provider value={theme}>
    <div style={{ ...row, width: '100%', height: '100%', alignItems: 'stretch', gap: 0, backgroundColor: theme.opaque ? '#0e0b13' : '#0e0b13dd' }}>
      <motion.div initial={false} animate={{ width: compact ? 72 : 224 }} transition={{ duration: theme.reducedMotion ? 0 : 0.18, ease: 'easeOut' }}
        style={{ ...column, height: '100%', flexShrink: 0, overflow: 'hidden', backgroundColor: theme.opaque ? '#18111c' : '#24192ce6', borderRightWidth: 1, borderColor: `${theme.accent}55` }}>
        <div style={{ ...row, height: 84, padding: 20, gap: 16 }}><Icon name="heart" color={theme.accent} size={28} />{!compact && <Label size={18} bold>DELTAMOD</Label>}</div>
        <div style={{ ...column, padding: 10, gap: 7, flexGrow: 1 }}>
          {routes.map(route => <div key={route.id} testId={`nav-${route.id}`} role="button" aria-label={route.label} aria-current={state.route === route.id ? 'page' : undefined} tabIndex={0}
            onClick={() => model.navigate(route.id)} onKeyDown={event => { if (event.key === 'enter' || event.key === 'space') model.navigate(route.id); }}
            style={{ ...row, padding: 13, gap: 16, height: 48, borderRadius: 13,
              backgroundColor: state.route === route.id ? `${theme.accent}33` : '#ffffff03',
              borderWidth: 1, borderColor: state.route === route.id ? `${theme.accent}66` : '#ffffff00', hover: { backgroundColor: '#ffffff14' } }}>
            <Icon name={route.icon} color={state.route === route.id ? theme.accent : colors.muted} />
            {!compact && <Label bold={state.route === route.id}>{route.label}</Label>}
          </div>)}
        </div>
        <div style={{ ...column, padding: 14 }}><Action onClick={() => setCollapsed(!collapsed)} testId="collapse-sidebar">{compact ? '>' : '< Collapse'}</Action>
          {!compact && <Label muted size={12}>GPUIX preview · read-only</Label>}</div>
      </motion.div>
      <div style={{ ...column, flexGrow: 1, minWidth: 0, minHeight: 0, gap: 0 }}>
        <div style={{ ...row, height: 66, padding: 16, paddingLeft: 28, justifyContent: 'space-between', flexShrink: 0 }}>
          <div style={row}><Chip active>Community</Chip><Label muted size={12}>Native GPUI rendering</Label></div>
          <div style={{ ...row, gap: 8 }}>
            <Action onClick={() => void model.refresh().catch(() => {})} disabled={state.loading}>Refresh</Action>
            <Action onClick={() => setModal({ kind: 'about' })}>About</Action>
            <Action onClick={() => renderer.minimizeWindow?.()}>Minimize</Action>
          </div>
        </div>
        {state.error && <div role="alert" style={{ padding: 14, backgroundColor: '#5d2035', margin: 12, borderRadius: 10 }}><Label>{state.error}</Label></div>}
        {!snapshot ? <Empty title="Backend unavailable">Restart the preview after building the native host.</Empty> : <>
          {state.route === 'home' && <Home snapshot={snapshot} attach={attach} navigate={route => model.navigate(route)} />}
          {state.route === 'library' && <Library snapshot={snapshot} show={item => setModal({ kind: 'mod', item })} attach={attach} />}
          {state.route === 'installations' && <Installations snapshot={snapshot} show={item => setModal({ kind: 'installation', item })} attach={attach} />}
          {state.route === 'shop' && <ShopView model={model} />}
          {state.route === 'themes' && <Page title="Your colors. Your Deltamod." subtitle="Built-in palettes, read from the existing theme catalogue.">
            <Label muted>Color preview only. Video, audio and custom CSS themes remain in Tauri for now.</Label>
            <virtual-list estimatedItemHeight={88} style={{ flexGrow: 1, minHeight: 0 }}>
              {snapshot.themes.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 14 }}>
                <div style={row}><div style={{ width: 30, height: 30, borderRadius: 15, backgroundColor: item.accent }} /><Label bold>{item.name}</Label></div>
                <Action disabled={state.saving} primary={prefs.themeId === item.id} onClick={() => void model.savePreferences({ themeId: item.id, accent: item.accent })}>{prefs.themeId === item.id ? 'Selected' : 'Use palette'}</Action>
              </GlassPanel></div>)}
            </virtual-list>
          </Page>}
          {state.route === 'settings' && <Page title="Make it yours" subtitle="These preferences belong only to the GPUIX preview.">
            <GlassPanel><Label size={18} bold>Appearance</Label>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>Reduced motion</Label><Action disabled={state.saving || overrides.reducedMotion !== null} onClick={() => void model.savePreferences({ reducedMotion: !prefs.reducedMotion })}>{theme.reducedMotion ? 'On' : 'Off'}</Action></div>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>Opaque surfaces</Label><Action disabled={state.saving || overrides.opaque !== null} onClick={() => void model.savePreferences({ opaque: !prefs.opaque })}>{theme.opaque ? 'On' : 'Off'}</Action></div>
              <Label muted size={12}>Window vibrancy applies after restart on macOS. Other platforms use layered translucent surfaces, not system liquid glass.</Label>
            </GlassPanel>
            <GlassPanel><Label size={18} bold>Profile safety</Label><Label muted>Profiles are attached read-only for this session. Changing preview settings does not modify your game files, mod state or Tauri preferences.</Label>
              <Action onClick={() => void attach()} disabled={state.loading} testId="attach-profile">Choose Deltamod data folder</Action>
            </GlassPanel>
          </Page>}
        </>}
      </div>
    </div>
    {modal && <InfoDialog close={() => setModal(null)} title={modal.kind === 'about' ? 'Deltamod · GPUIX preview' : modal.item.name}
      description={modal.kind === 'about' ? 'React and GPUIX with a separate Rust domain host. No Tauri window or WebView is started.' : modal.kind === 'mod' ? modal.item.description || 'No description is available.' : modal.item.path}>
      {modal.kind === 'mod' && <><Chip>{modal.item.format}</Chip><Label muted>Enable, remove, import and patch actions are deliberately unavailable until transactional parity is verified.</Label></>}
      {modal.kind === 'installation' && <><Label>{modal.item.gameId}</Label><Label muted>{'The saved game path is shown without probing or modifying it.'}</Label></>}
      {modal.kind === 'about' && <Label muted>Experimental, not a replacement release. Tauri remains the production application.</Label>}
    </InfoDialog>}
  </Palette.Provider>;
}
function Home({ snapshot, attach, navigate }: { snapshot: Snapshot; attach: () => Promise<void>; navigate: (route: Route) => void }) {
  return <Page testId="home-ready" title="Your games, together." subtitle="A lighter native home for Deltamod Community.">
    <GlassPanel style={{ padding: 28, gap: 18 }}><Label size={22} bold>{snapshot.sourceAttached ? 'Your library is connected' : 'Bring your library into view'}</Label>
      <Label muted>{snapshot.sourceAttached ? 'This preview reads your existing profile without changing it.' : 'Choose your Deltamod data folder to see installations and mods. No copying, migration or game changes.'}</Label>
      <div style={row}><Action primary onClick={() => void attach()}>Choose profile</Action><Action onClick={() => navigate('shop')}>Explore Mod Shop</Action></div>
    </GlassPanel>
    <div style={{ ...row, alignItems: 'stretch' }}>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>Installations</Label><Label size={34} bold>{snapshot.installations.length}</Label></GlassPanel>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>Mods</Label><Label size={34} bold>{snapshot.mods.length}</Label></GlassPanel>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>Theme palettes</Label><Label size={34} bold>{snapshot.themes.length}</Label></GlassPanel>
    </div>
    {snapshot.warnings.length > 0 && <GlassPanel><Label bold>Some data needs attention</Label>{snapshot.warnings.map((warning, i) => <Label key={i} muted>{warning}</Label>)}</GlassPanel>}
  </Page>;
}
function Library({ snapshot, show, attach }: { snapshot: Snapshot; show: (mod: Mod) => void; attach: () => Promise<void> }) {
  const [query, setQuery] = useState('');
  const mods = useMemo(() => filterMods(snapshot.mods, query), [snapshot.mods, query]);
  return <Page title="Mod library" subtitle="Your existing mods, without changing their enabled state.">
    <input testId="library-search" value={query} placeholder="Search your mods" onChange={event => setQuery(event.value || '')}
      style={{ height: 44, padding: 12, borderRadius: 12, backgroundColor: '#231c2b', color: colors.text, fontSize: 14 }} />
    {!snapshot.sourceAttached ? <><Empty title="No profile attached">Attach your Deltamod data folder to inspect its library.</Empty><Action onClick={() => void attach()}>Choose profile</Action></>
      : mods.length === 0 ? <Empty title="No matching mods">Try another search, or import mods in the current Tauri application.</Empty>
      : <virtual-list estimatedItemHeight={96} style={{ flexGrow: 1, minHeight: 0 }}>{mods.map((mod, i) => <div key={`${mod.format}:${mod.id}:${i}`} style={{ paddingBottom: 10 }}>
        <GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 16 }}><div style={{ ...column, gap: 7, flexGrow: 1, minWidth: 0 }}><Label bold>{mod.name}</Label>
          <div style={row}><Chip active={mod.enabled === true}>{mod.enabled === null ? 'State unavailable' : mod.enabled ? 'Enabled' : 'Disabled'}</Chip><Label muted size={12}>{mod.format}</Label></div></div>
          <Action onClick={() => show(mod)}>Details</Action>
        </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function Installations({ snapshot, show, attach }: { snapshot: Snapshot; show: (installation: Installation) => void; attach: () => Promise<void> }) {
  return <Page title="Installations" subtitle="Existing installation records. No repair, launch or file writes in this preview.">
    {snapshot.installations.length === 0 ? <><Empty title="No installations to display">Choose a Deltamod data folder containing your configured installations.</Empty><Action onClick={() => void attach()}>Choose profile</Action></>
      : <virtual-list estimatedItemHeight={100} style={{ flexGrow: 1, minHeight: 0 }}>{snapshot.installations.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
        <div style={column}><Label bold>{item.name || item.gameId}</Label><div style={row}><Chip active={item.current}>{item.current ? 'Current' : 'Installation'}</Chip><Label muted size={12}>{item.available === null ? 'Saved path · not checked' : item.available ? 'Folder found' : 'Folder missing'}</Label></div></div>
        <Action onClick={() => show(item)}>Details</Action>
      </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function ShopView({ model }: { model: AppModel }) {
  const { shop } = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const [query, setQuery] = useState(shop.query);
  function open(url: string) { void openModPage(url).catch(error => model.update({ error: String(error.message || error) })); }
  return <Page title="Find your next mod" subtitle="DELTARUNE · GameBanana. Powered by the existing Rust network runtime.">
    <div style={row}><input testId="shop-search" value={query} placeholder="Search public mods" onChange={event => setQuery(event.value || '')} onSubmit={() => void model.browse(query)}
      style={{ height: 44, padding: 12, flexGrow: 1, color: colors.text, fontSize: 14, backgroundColor: '#231c2b', borderRadius: 12 }} />
      <Action primary disabled={shop.status === 'loading'} onClick={() => void model.browse(query)}>Search</Action></div>
    {shop.status === 'loading' && <Label muted>Loading GameBanana...</Label>}
    {shop.status === 'error' && <GlassPanel><Label>{shop.error}</Label><Action onClick={() => void model.browse(shop.query, shop.page)}>Retry</Action></GlassPanel>}
    {shop.status === 'ready' && shop.items.length === 0 && <Empty title="No results">Try a different search.</Empty>}
    <virtual-list estimatedItemHeight={108} style={{ flexGrow: 1, minHeight: 0 }}>{shop.items.map((item, i) => <div key={`${item.id}:${i}`} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
      <div style={{ ...column, gap: 6, minWidth: 0, flexGrow: 1 }}><Label bold>{item.name}</Label><Label muted size={12}>{item.author || 'GameBanana'}</Label></div>
      <Action onClick={() => open(item.url)}>View mod</Action>
    </GlassPanel></div>)}</virtual-list>
    <div style={{ ...row, justifyContent: 'space-between' }}><Label muted>Page {shop.page} · Downloads and account login remain in Tauri.</Label><div style={row}>
      <Action disabled={shop.status === 'loading' || shop.page <= 1} onClick={() => void model.browse(shop.query, shop.page - 1)}>Previous</Action>
      <Action disabled={shop.status !== 'ready' || !shop.hasMore} onClick={() => void model.browse(shop.query, shop.page + 1)}>Next</Action>
    </div></div>
  </Page>;
}
