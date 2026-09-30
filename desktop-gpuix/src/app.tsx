// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useState, useSyncExternalStore, useEffect, useMemo, useRef, type RefObject } from 'react';
import { motion, useGpuixRequired, useWindowSize, type PublicInstance } from '@gpuix/react';
import type { AppModel } from './model.mjs';
import { queryMods, DEFAULT_LIBRARY } from './model.mjs';
import { openModPage } from './options.mjs';
import type { Mod, Installation, Preferences, Route, Snapshot, LibraryQuery } from './contracts.js';
import { Palette, colors, row, column, Label, Action, GlassPanel, Page, Empty, Chip, InfoDialog } from './ui.js';

type Modal = { kind: 'mod'; item: Mod } | { kind: 'installation'; item: Installation } | { kind: 'about' | 'help' | 'games' | 'detach' } | null;
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
  const searchRef = useRef<PublicInstance | null>(null);
  const pickerOpen = useRef(false);
  const [searchFocus, setSearchFocus] = useState(0);
  const snapshot = state.snapshot;
  const busy = state.loading || state.saving || state.shop.status === 'loading';
  const prefs = snapshot?.preferences || { themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true };
  const theme: Preferences = { ...prefs, reducedMotion: overrides.reducedMotion ?? prefs.reducedMotion, opaque: overrides.opaque ?? prefs.opaque };
  const compact = collapsed || size.width < 1000;
  useEffect(() => { if (snapshot) onCommitted(); }, [snapshot, onCommitted]);
  useEffect(() => {
    if (state.route === 'shop' && state.shop.status === 'idle' && state.shop.gameId && !state.loading) void model.browse('');
  }, [state.route, state.shop.status, state.shop.gameId, state.loading, model]);
  useEffect(() => { setModal(null); }, [state.profileEpoch]);
  useEffect(() => {
    if (searchFocus && searchRef.current) renderer.focusElement(searchRef.current.id);
  }, [searchFocus, state.route, renderer]);
  useEffect(() => model.onShortcut(shortcut => {
    // DialogPopup owns modal Tab traversal. Never also advance window focus.
    if (modal) { if (shortcut.action === 'close') setModal(null); return; }
    const current = model.state;
    const working = current.loading || current.saving || current.shop.status === 'loading';
    switch (shortcut.action) {
      case 'tab-next': renderer.focusNext?.(); break;
      case 'tab-previous': renderer.focusPrevious?.(); break;
      case 'navigate': model.navigate(shortcut.route); break;
      case 'back': model.goBack(); break;
      case 'forward': model.goForward(); break;
      case 'refresh': if (!working) void model.refresh().catch(() => {}); break;
      case 'attach': if (!working) void attach(); break;
      case 'help': setModal({ kind: 'help' }); break;
      case 'search':
        if (current.route !== 'library' && current.route !== 'shop') model.navigate('library');
        setSearchFocus(value => value + 1);
        break;
    }
  }), [model, renderer, modal]);

  async function attach() {
    if (pickerOpen.current || model.state.loading || model.state.saving || model.state.shop.status === 'loading') return;
    pickerOpen.current = true;
    try {
      if (!renderer.promptForPaths) throw new Error('Native folder picker is unavailable on this runtime.');
      const files = await renderer.promptForPaths({ files: false, directories: true, multiple: false, prompt: 'Read a Deltamod profile' });
      if (files?.[0]) await model.attachProfile(files[0]);
    } catch (error) { model.update({ error: error instanceof Error ? error.message : String(error) }); }
    finally { pickerOpen.current = false; }
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
            <Action testId="history-back" disabled={!state.history.back.length} onClick={() => model.goBack()}>Back</Action>
            <Action testId="history-forward" disabled={!state.history.forward.length} onClick={() => model.goForward()}>Forward</Action>
            <Action onClick={() => void model.refresh().catch(() => {})} disabled={busy}>Refresh</Action>
            <Action testId="about-preview" onClick={() => setModal({ kind: 'about' })}>About</Action>
            <Action onClick={() => renderer.minimizeWindow?.()}>Minimize</Action>
          </div>
        </div>
        {state.error && <div role="alert" style={{ padding: 14, backgroundColor: '#5d2035', margin: 12, borderRadius: 10 }}><Label>{state.error}</Label></div>}
        {!snapshot ? <Empty title="Backend unavailable">Restart the preview after building the native host.</Empty> : <>
          {state.route === 'home' && <Home snapshot={snapshot} attach={attach} navigate={route => model.navigate(route)} />}
          {state.route === 'library' && <Library model={model} searchRef={searchRef} snapshot={snapshot} show={item => setModal({ kind: 'mod', item })} attach={attach} />}
          {state.route === 'installations' && <Installations model={model} snapshot={snapshot} show={item => setModal({ kind: 'installation', item })} attach={attach} />}
          {state.route === 'shop' && <ShopView model={model} searchRef={searchRef} chooseGame={() => setModal({ kind: 'games' })} />}
          {state.route === 'themes' && <Page title="Your colors. Your Deltamod." subtitle="Built-in palettes, read from the existing theme catalogue.">
            <Label muted>Color preview only. Video, audio and custom CSS themes remain in Tauri for now.</Label>
            <virtual-list estimatedItemHeight={88} style={{ flexGrow: 1, minHeight: 0 }}>
              {snapshot.themes.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 14 }}>
                <div style={row}><div style={{ width: 30, height: 30, borderRadius: 15, backgroundColor: item.accent }} /><Label bold>{item.name}</Label></div>
                <Action disabled={busy} primary={prefs.themeId === item.id} onClick={() => void model.savePreferences({ themeId: item.id, accent: item.accent })}>{prefs.themeId === item.id ? 'Selected' : 'Use palette'}</Action>
              </GlassPanel></div>)}
            </virtual-list>
          </Page>}
          {state.route === 'settings' && <Page title="Make it yours" subtitle="These preferences belong only to the GPUIX preview.">
            <GlassPanel><Label size={18} bold>Appearance</Label>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>Reduced motion</Label><Action disabled={busy || overrides.reducedMotion !== null} onClick={() => void model.savePreferences({ reducedMotion: !prefs.reducedMotion })}>{theme.reducedMotion ? 'On' : 'Off'}</Action></div>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>Opaque surfaces</Label><Action disabled={busy || overrides.opaque !== null} onClick={() => void model.savePreferences({ opaque: !prefs.opaque })}>{theme.opaque ? 'On' : 'Off'}</Action></div>
              <Label muted size={12}>Window vibrancy applies after restart on macOS. Other platforms use layered translucent surfaces, not system liquid glass.</Label>
            </GlassPanel>
            <GlassPanel><Label size={18} bold>Profile safety</Label><Label muted>Profiles are attached read-only for this session. Changing preview settings does not modify your game files, mod state or Tauri preferences.</Label>
              <div style={row}>
                <Action onClick={() => void attach()} disabled={busy} testId="attach-profile">Choose Deltamod data folder</Action>
                <Action onClick={() => setModal({ kind: 'detach' })} disabled={busy || !snapshot.sourceAttached} testId="detach-profile">Disconnect profile</Action>
              </div>
              <Label muted size={12}>Installation selections last for this session and never change Tauri's current installation.</Label>
              <Action onClick={() => setModal({ kind: 'help' })}>Keyboard shortcuts</Action>
            </GlassPanel>
          </Page>}
        </>}
      </div>
    </div>
    {modal && <InfoDialog close={() => setModal(null)}
      title={modal.kind === 'mod' || modal.kind === 'installation' ? modal.item.name :
        modal.kind === 'help' ? 'Keyboard shortcuts' : modal.kind === 'games' ? 'Choose a game' : modal.kind === 'detach' ? 'Disconnect this profile?' : 'Deltamod · GPUIX preview'}
      description={modal.kind === 'mod' ? modal.item.description || 'No description is available.' :
        modal.kind === 'installation' ? modal.item.path : modal.kind === 'detach' ? 'This clears the preview library and session selection. No files are deleted or modified.' :
        modal.kind === 'games' ? "Games come from Deltamod's packaged catalogue, not arbitrary URLs." :
        modal.kind === 'help' ? 'Use Cmd on macOS and Ctrl on Windows/Linux.' : 'React and GPUIX with a separate Rust domain host. No Tauri window or WebView is started.'}>
      {modal.kind === 'mod' && <><Chip>{modal.item.format}</Chip><Label muted>Enable, remove, import and patch actions are deliberately unavailable until transactional parity is verified.</Label></>}
      {modal.kind === 'installation' && <><Label>{modal.item.gameId}</Label><Label muted>The saved game path is shown without probing or modifying it.</Label></>}
      {modal.kind === 'about' && <Label muted>Experimental, not a replacement release. Tauri remains the production application.</Label>}
      {modal.kind === 'help' && <>
        <Label>Ctrl/Cmd + 1–6: switch screens</Label><Label>Alt + Left/Right: back/forward</Label>
        <Label>Ctrl/Cmd + F: focus search</Label><Label>Ctrl/Cmd + R: refresh profile</Label>
        <Label>Ctrl/Cmd + Shift + O: attach profile</Label><Label>F1: shortcuts · Escape: close dialog</Label>
        <Label muted>Tab stays inside an open dialog. Held keys do not repeat application shortcuts.</Label>
      </>}
      {modal.kind === 'detach' && <Action testId="confirm-detach-profile" disabled={busy} primary onClick={() => {
        void model.detachProfile().then(done => { if (done) setModal(null); });
      }}>Disconnect profile</Action>}
      {modal.kind === 'games' && <virtual-list estimatedItemHeight={64} style={{ height: 290 }}>
        {snapshot?.games.map(game => <div key={game.id} style={{ ...row, justifyContent: 'space-between', padding: 10 }}>
          <Label>{game.name}</Label><Action testId={`shop-game-${game.id}`} disabled={busy || !game.gamebanana} primary={state.shop.gameId === game.id}
            onClick={() => { model.setShopGame(game.id); setModal(null); }}>{game.gamebanana ? 'Browse' : 'Unavailable'}</Action>
        </div>)}
      </virtual-list>}
    </InfoDialog>}
  </Palette.Provider>;
}
function Home({ snapshot, attach, navigate }: { snapshot: Snapshot; attach: () => Promise<void>; navigate: (route: Route) => void }) {
  const selected = snapshot.installations.find(item => item.id === snapshot.selectedInstallationId);
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
    {selected && <GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
      <div style={column}><Label muted>Preview installation</Label><Label bold>{selected.name || selected.gameId}</Label></div>
      <Action onClick={() => navigate('installations')}>Change selection</Action>
    </GlassPanel>}
    {snapshot.warnings.length > 0 && <GlassPanel><Label bold>Some data needs attention</Label>{snapshot.warnings.map((warning, i) => <Label key={i} muted>{warning}</Label>)}</GlassPanel>}
  </Page>;
}
function Library({ model, snapshot, show, attach, searchRef }: { model: AppModel; snapshot: Snapshot; show: (mod: Mod) => void; attach: () => Promise<void>; searchRef: RefObject<PublicInstance | null> }) {
  const { library } = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const mods = useMemo(() => queryMods(snapshot.mods, library), [snapshot.mods, library]);
  const formats: LibraryQuery['format'][] = ['all', 'runtime', 'legacy packet'];
  const sorts: LibraryQuery['sort'][] = ['name-asc', 'name-desc', 'enabled-first'];
  return <Page title="Mod library" subtitle="Profile-wide library. Installation selection does not change mod enabled states.">
    <input ref={searchRef} testId="library-search" value={library.query} placeholder="Search name, description or ID" onChange={event => model.setLibrary({ query: event.value || '' })}
      style={{ height: 44, padding: 12, borderRadius: 12, backgroundColor: '#231c2b', color: colors.text, fontSize: 14 }} />
    <div style={{ ...column, gap: 8 }}>
      <div style={row}>{(['all', 'enabled', 'disabled', 'unknown'] as const).map(status =>
        <Action key={status} testId={`library-status-${status}`} primary={library.status === status} onClick={() => model.setLibrary({ status })}>
          {status === 'unknown' ? 'Unknown state' : status === 'all' ? 'All states' : status === 'enabled' ? 'Enabled' : 'Disabled'}
        </Action>)}</div>
      <div style={row}>
        <Action testId="library-format" onClick={() => model.setLibrary({ format: formats[(formats.indexOf(library.format) + 1) % formats.length]! })}>Format: {library.format}</Action>
        <Action testId="library-sort" onClick={() => model.setLibrary({ sort: sorts[(sorts.indexOf(library.sort) + 1) % sorts.length]! })}>
          {library.sort === 'name-asc' ? 'Name A–Z' : library.sort === 'name-desc' ? 'Name Z–A' : 'Enabled first'}
        </Action>
        <Action testId="library-reset" onClick={() => model.setLibrary(DEFAULT_LIBRARY)}>Reset</Action>
        <Label muted>{mods.length} of {snapshot.mods.length}</Label>
      </div>
    </div>
    {!snapshot.sourceAttached ? <><div testId="profile-detached"><Empty title="No profile attached">Attach your Deltamod data folder to inspect its library.</Empty></div><Action onClick={() => void attach()}>Choose profile</Action></>
      : mods.length === 0 ? <Empty title="No matching mods">Try another search, or import mods in the current Tauri application.</Empty>
      : <virtual-list estimatedItemHeight={96} style={{ flexGrow: 1, minHeight: 0 }}>{mods.map((mod, i) => <div key={`${mod.format}:${mod.id}:${i}`} testId={`library-mod-${mod.id}`} style={{ paddingBottom: 10 }}>
        <GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 16 }}><div style={{ ...column, gap: 7, flexGrow: 1, minWidth: 0 }}><Label bold>{mod.name}</Label>
          <div style={row}><Chip active={mod.enabled === true}>{mod.enabled === null ? 'State unavailable' : mod.enabled ? 'Enabled' : 'Disabled'}</Chip><Label muted size={12}>{mod.format}</Label></div></div>
          <Action onClick={() => show(mod)}>Details</Action>
        </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function Installations({ model, snapshot, show, attach }: { model: AppModel; snapshot: Snapshot; show: (installation: Installation) => void; attach: () => Promise<void> }) {
  const busy = model.state.loading || model.state.saving || model.state.shop.status === 'loading';
  return <Page title="Installations" subtitle="Choose a preview context. Tauri's current installation and game files stay unchanged.">
    {snapshot.installations.length === 0 ? <><Empty title="No installations to display">Choose a Deltamod data folder containing your configured installations.</Empty><Action onClick={() => void attach()}>Choose profile</Action></>
      : <virtual-list estimatedItemHeight={100} style={{ flexGrow: 1, minHeight: 0 }}>{snapshot.installations.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
        <div style={column}><Label bold>{item.name || item.gameId}</Label><div style={row}><div testId={item.selected ? `preview-selected-${item.id}` : undefined}><Chip active={item.selected}>{item.selected ? 'Preview selection' : item.current ? 'Tauri current' : 'Installation'}</Chip></div><Label muted size={12}>{item.available === null ? 'Saved path · not checked' : item.available ? 'Folder found' : 'Folder missing'}</Label></div></div>
        <div style={row}><Action testId={`select-installation-${item.id}`} primary={item.selected} disabled={busy || item.selected}
          onClick={() => void model.selectInstallation(item.id)}>{item.selected ? 'Selected' : 'Select'}</Action>
          <Action testId={`browse-installation-${item.id}`} disabled={busy} onClick={() => {
            void model.selectInstallation(item.id).then(done => { if (done) model.navigate('shop'); });
          }}>Browse mods</Action><Action onClick={() => show(item)}>Details</Action></div>
      </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function ShopView({ model, searchRef, chooseGame }: { model: AppModel; searchRef: RefObject<PublicInstance | null>; chooseGame: () => void }) {
  const { shop, snapshot, loading } = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const [query, setQuery] = useState(shop.query);
  useEffect(() => { setQuery(shop.query); }, [shop.gameId, shop.query]);
  const game = snapshot?.games.find(item => item.id === shop.gameId);
  function open(url: string) { void openModPage(url).catch(error => model.update({ error: String(error.message || error) })); }
  return <Page title="Find your next mod" subtitle={`${game?.name || 'Choose a game'} · GameBanana. Powered by the existing Rust network runtime.`}>
    <div style={row}><Action testId="shop-game-picker" disabled={loading || shop.status === 'loading'} onClick={chooseGame}>{game?.name || 'Choose game'}</Action>
      <Label muted>Library selection is session-only.</Label></div>
    {!shop.gameId && <Empty title="No Mod Shop mapping">Choose a supported game from the catalogue. A missing mapping is not replaced with another game's results.</Empty>}
    <div style={row}><input ref={searchRef} testId="shop-search" value={query} placeholder="Search public mods" onChange={event => setQuery(event.value || '')} onSubmit={() => { if (shop.status !== 'loading') void model.browse(query); }}
      style={{ height: 44, padding: 12, flexGrow: 1, color: colors.text, fontSize: 14, backgroundColor: '#231c2b', borderRadius: 12 }} />
      <Action primary disabled={loading || !shop.gameId || shop.status === 'loading'} onClick={() => void model.browse(query)}>Search</Action></div>
    {shop.status === 'loading' && <Label muted>Loading GameBanana...</Label>}
    {shop.status === 'error' && <GlassPanel><Label>{shop.error}</Label><Action onClick={() => void model.browse(shop.query, shop.page)}>Retry</Action></GlassPanel>}
    {shop.status === 'ready' && shop.items.length === 0 && <Empty title="No results">Try a different search.</Empty>}
    <virtual-list estimatedItemHeight={108} style={{ flexGrow: 1, minHeight: 0 }}>{shop.items.map((item, i) => <div key={`${item.id}:${i}`} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
      <div style={{ ...column, gap: 6, minWidth: 0, flexGrow: 1 }}><Label bold>{item.name}</Label><Label muted size={12}>{item.author || 'GameBanana'}</Label></div>
      <Action onClick={() => open(item.url)}>View mod</Action>
    </GlassPanel></div>)}</virtual-list>
    <div style={{ ...row, justifyContent: 'space-between' }}><Label muted>Page {shop.page} · Downloads and account login remain in Tauri.</Label><div style={row}>
      <Action disabled={loading || shop.status !== 'ready' || shop.page <= 1} onClick={() => void model.browse(shop.query, shop.page - 1)}>Previous</Action>
      <Action disabled={loading || shop.status !== 'ready' || !shop.hasMore || shop.page >= 100} onClick={() => void model.browse(shop.query, shop.page + 1)}>Next</Action>
    </div></div>
  </Page>;
}
