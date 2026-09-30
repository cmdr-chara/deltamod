// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useState, useSyncExternalStore, useEffect, useMemo, useRef, type RefObject } from 'react';
import { motion, useGpuixRequired, useWindowSize, type PublicInstance } from '@gpuix/react';
import type { AppModel } from './model.mjs';
import { queryMods, DEFAULT_LIBRARY } from './model.mjs';
import type { DesktopFeatures } from './features.mjs';
import type { ManagedRuntime } from './managed.mjs';
import { ManagedLibraryPanel, ManagedSystemPanel, ProtocolImportDialog } from './managed-ui.js';
import type { UpdateRuntime } from './updater.mjs';
import { UpdatePanel } from './updater.js';
import { LanguageContext, useMessages } from './i18n.js';
import { ThemePreviewPanel, PresentationSettings, ModDetailDialog, LinkDialog } from './presentation.js';
import type { Mod, Installation, Preferences, Route, Snapshot, LibraryQuery } from './contracts.js';
import { Palette, colors, row, column, Label, Action, GlassPanel, Page, Empty, Chip, InfoDialog } from './ui.js';

type Modal = { kind: 'mod'; item: Mod } | { kind: 'installation'; item: Installation } | { kind: 'about' | 'help' | 'games' | 'detach' | 'link' } | null;
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

interface AppProps { model: AppModel; features: DesktopFeatures; managed: ManagedRuntime; updater: UpdateRuntime; overrides: { reducedMotion: boolean | null; opaque: boolean | null }; onCommitted: () => void }
export function App(props: AppProps) {
  const presentation = useSyncExternalStore(props.features.subscribe, props.features.getSnapshot);
  return <LanguageContext.Provider value={presentation.preferences.locale}><AppContent {...props} /></LanguageContext.Provider>;
}
function AppContent({ model, features, managed, updater, overrides, onCommitted }: AppProps) {
  const t = useMessages();
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const presentation = useSyncExternalStore(features.subscribe, features.getSnapshot);
  const managedState = useSyncExternalStore(managed.subscribe, managed.getSnapshot);
  const renderer = useGpuixRequired();
  const size = useWindowSize();
  const [collapsed, setCollapsed] = useState(false);
  const [modal, setModal] = useState<Modal>(null);
  const searchRef = useRef<PublicInstance | null>(null);
  const pickerOpen = useRef(false);
  const [searchFocus, setSearchFocus] = useState(0);
  const snapshot = state.snapshot;
  const busy = state.loading || state.saving || state.shop.status === 'loading' || presentation.saving || presentation.detail.status === 'loading' || managedState.loading || !!managedState.busy;
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
    if (modal || presentation.detail.status !== 'idle' || presentation.pendingLink || managedState.protocol.status !== 'idle') {
      if (shortcut.action === 'close') {
        setModal(null); features.closeDetail(); features.dismissLink();
        if (managedState.protocol.status === 'importing') void managed.cancelProtocol();
        else managed.dismissProtocol();
      }
      return;
    }
    const current = model.state;
    const working = current.loading || current.saving || current.shop.status === 'loading' || features.state.saving;
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
  }), [model, renderer, modal, features, managed, managedState.protocol.status, presentation.detail.status, presentation.pendingLink, t]);

  async function attach() {
    if (pickerOpen.current || model.state.loading || model.state.saving || model.state.shop.status === 'loading' || features.state.saving || features.state.detail.status === 'loading') return;
    pickerOpen.current = true;
    try {
      if (!renderer.promptForPaths) throw new Error('Native folder picker is unavailable on this runtime.');
      const files = await renderer.promptForPaths({ files: false, directories: true, multiple: false, prompt: t('Read a Deltamod profile') });
      if (files?.[0]) await model.attachProfile(files[0]);
    } catch (error) { model.update({ error: error instanceof Error ? error.message : String(error) }); }
    finally { pickerOpen.current = false; }
  }
  return <Palette.Provider value={theme}>
    <div style={{ ...row, width: '100%', height: '100%', alignItems: 'stretch', gap: 0, backgroundColor: theme.opaque ? '#0e0b13' : '#0e0b13dd' }}>
      <motion.div initial={false} animate={{ width: compact ? 72 : 224 }} transition={{ duration: theme.reducedMotion ? 0 : 0.18, ease: 'easeOut' }}
        style={{ ...column, height: '100%', flexShrink: 0, overflow: 'hidden', backgroundColor: theme.opaque ? '#18111c' : '#24192ce6', borderRightWidth: 1, borderColor: `${theme.accent}55` }}>
        <div style={{ ...row, height: 84, padding: 20, gap: 16 }}><Icon name="heart" color={theme.accent} size={28} />{!compact && <Label size={18} bold>{t("DELTAMOD")}</Label>}</div>
        <div style={{ ...column, padding: 10, gap: 7, flexGrow: 1 }}>
          {routes.map(route => <div key={route.id} testId={`nav-${route.id}`} role="button" aria-label={t(route.label)} aria-current={state.route === route.id ? 'page' : undefined} tabIndex={0}
            onClick={() => model.navigate(route.id)} onKeyDown={event => { if (event.key === 'enter' || event.key === 'space') model.navigate(route.id); }}
            style={{ ...row, padding: 13, gap: 16, height: 48, borderRadius: 13,
              backgroundColor: state.route === route.id ? `${theme.accent}33` : '#ffffff03',
              borderWidth: 1, borderColor: state.route === route.id ? `${theme.accent}66` : '#ffffff00', hover: { backgroundColor: '#ffffff14' } }}>
            <Icon name={route.icon} color={state.route === route.id ? theme.accent : colors.muted} />
            {!compact && <Label bold={state.route === route.id}>{t(route.label)}</Label>}
          </div>)}
        </div>
        <div style={{ ...column, padding: 14 }}><Action onClick={() => setCollapsed(!collapsed)} testId="collapse-sidebar">{compact ? '>' : t("< Collapse")}</Action>
          {!compact && <Label muted size={12}>{t("GPUIX preview · read-only")}</Label>}</div>
      </motion.div>
      <div style={{ ...column, flexGrow: 1, minWidth: 0, minHeight: 0, gap: 0 }}>
        <div style={{ ...row, height: 66, padding: 16, paddingLeft: 28, justifyContent: 'space-between', flexShrink: 0 }}>
          <div style={row}><Chip active>{t("Community")}</Chip><Label muted size={12}>{t("Native GPUI rendering")}</Label></div>
          <div style={{ ...row, gap: 8 }}>
            <Action testId="history-back" disabled={!state.history.back.length} onClick={() => model.goBack()}>{t("Back")}</Action>
            <Action testId="history-forward" disabled={!state.history.forward.length} onClick={() => model.goForward()}>{t("Forward")}</Action>
            <Action onClick={() => void model.refresh().catch(() => {})} disabled={busy}>{t("Refresh")}</Action>
            <Action testId="about-preview" onClick={() => setModal({ kind: 'about' })}>{t("About")}</Action>
            <Action onClick={() => renderer.minimizeWindow?.()}>{t("Minimize")}</Action>
          </div>
        </div>
        {presentation.error && !presentation.pendingLink && modal?.kind !== 'link' && <div role="alert" style={{ padding: 14, backgroundColor: '#5d2035', margin: 12, borderRadius: 10 }}><Label>{t(presentation.error)}</Label><Action onClick={() => features.update({ error: '' })}>{t('Dismiss')}</Action></div>}
        {state.error && <div role="alert" style={{ padding: 14, backgroundColor: '#5d2035', margin: 12, borderRadius: 10 }}><Label>{state.error}</Label></div>}
        {!snapshot ? <Empty title={t("Backend unavailable")}>{t("Restart the preview after building the native host.")}</Empty> : <>
          {state.route === 'home' && <Home features={features} snapshot={snapshot} attach={attach} navigate={route => model.navigate(route)} />}
          {state.route === 'library' && <Library managed={managed} model={model} searchRef={searchRef} snapshot={snapshot} show={item => setModal({ kind: 'mod', item })} attach={attach} />}
          {state.route === 'installations' && <Installations model={model} snapshot={snapshot} show={item => setModal({ kind: 'installation', item })} attach={attach} />}
          {state.route === 'shop' && <ShopView features={features} model={model} searchRef={searchRef} chooseGame={() => setModal({ kind: 'games' })} />}
          {state.route === 'themes' && <Page title={t("Your colors. Your Deltamod.")} subtitle={t("Built-in palettes, read from the existing theme catalogue.")}>
            <Label muted>{t("Video, audio and custom CSS themes remain in Tauri for now.")}</Label>
            <ThemePreviewPanel features={features} themeId={prefs.themeId} />
            <virtual-list estimatedItemHeight={88} style={{ flexGrow: 1, minHeight: 0 }}>
              {snapshot.themes.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 14 }}>
                <div style={row}><div style={{ width: 30, height: 30, borderRadius: 15, backgroundColor: item.accent }} /><Label bold>{item.name}</Label></div>
                <Action disabled={busy} primary={prefs.themeId === item.id} onClick={() => void model.savePreferences({ themeId: item.id, accent: item.accent })}>{prefs.themeId === item.id ? t("Selected") : t("Use palette")}</Action>
              </GlassPanel></div>)}
            </virtual-list>
          </Page>}
          {state.route === 'settings' && <Page title={t("Make it yours")} subtitle={t("These preferences belong only to the GPUIX preview.")}>
            <div style={{ ...column, flexGrow: 1, minHeight: 0, overflowY: 'scroll' }}>
            <PresentationSettings features={features} disabled={busy} openLink={() => { features.update({ error: '' }); setModal({ kind: 'link' }); }} />
            <ManagedSystemPanel runtime={managed} />
            <UpdatePanel runtime={updater} />
            <GlassPanel><Label size={18} bold>{t("Appearance")}</Label>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>{t("Reduced motion")}</Label><Action disabled={busy || overrides.reducedMotion !== null} onClick={() => void model.savePreferences({ reducedMotion: !prefs.reducedMotion })}>{theme.reducedMotion ? t("On") : t("Off")}</Action></div>
              <div style={{ ...row, justifyContent: 'space-between' }}><Label>{t("Opaque surfaces")}</Label><Action disabled={busy || overrides.opaque !== null} onClick={() => void model.savePreferences({ opaque: !prefs.opaque })}>{theme.opaque ? t("On") : t("Off")}</Action></div>
              <Label muted size={12}>{t("Window vibrancy applies after restart on macOS. Other platforms use layered translucent surfaces, not system liquid glass.")}</Label>
            </GlassPanel>
            <GlassPanel><Label size={18} bold>{t("Profile safety")}</Label><Label muted>{t("Profiles are attached read-only for this session. Changing preview settings does not modify your game files, mod state or Tauri preferences.")}</Label>
              <div style={row}>
                <Action onClick={() => void attach()} disabled={busy} testId="attach-profile">{t("Choose Deltamod data folder")}</Action>
                <Action onClick={() => setModal({ kind: 'detach' })} disabled={busy || !snapshot.sourceAttached} testId="detach-profile">{t("Disconnect profile")}</Action>
              </div>
              <Label muted size={12}>{t("Installation selections last for this session and never change Tauri's current installation.")}</Label>
              <Action onClick={() => setModal({ kind: 'help' })}>{t("Keyboard shortcuts")}</Action>
            </GlassPanel>
            </div>
          </Page>}
        </>}
      </div>
    </div>
    {modal && modal.kind !== 'link' && <InfoDialog close={() => setModal(null)}
      title={modal.kind === 'mod' || modal.kind === 'installation' ? modal.item.name :
        modal.kind === 'help' ? t("Keyboard shortcuts") : modal.kind === 'games' ? t("Choose a game") : modal.kind === 'detach' ? t("Disconnect this profile?") : t("Deltamod · GPUIX preview")}
      description={modal.kind === 'mod' ? modal.item.description || t("No description is available.") :
        modal.kind === 'installation' ? modal.item.path : modal.kind === 'detach' ? t("This clears the preview library and session selection. No files are deleted or modified.") :
        modal.kind === 'games' ? t("Games come from Deltamod's packaged catalogue, not arbitrary URLs.") :
        modal.kind === 'help' ? t("Use Cmd on macOS and Ctrl on Windows/Linux.") : t("React and GPUIX with a separate Rust domain host. No Tauri window or WebView is started.")}>
      {modal.kind === 'mod' && <><Chip>{t(`format.${modal.item.format}`)}</Chip><Label muted>{t("Enable, remove, import and patch actions are deliberately unavailable until transactional parity is verified.")}</Label></>}
      {modal.kind === 'installation' && <><Label>{modal.item.gameId}</Label><Label muted>{t("The saved game path is shown without probing or modifying it.")}</Label></>}
      {modal.kind === 'about' && <Label muted>{t("Experimental, not a replacement release. Tauri remains the production application.")}</Label>}
      {modal.kind === 'help' && <>
        <Label>{t("Ctrl/Cmd + 1–6: switch screens")}</Label><Label>{t("Alt + Left/Right: back/forward")}</Label>
        <Label>{t("Ctrl/Cmd + F: focus search")}</Label><Label>{t("Ctrl/Cmd + R: refresh profile")}</Label>
        <Label>{t("Ctrl/Cmd + Shift + O: attach profile")}</Label><Label>{t("F1: shortcuts · Escape: close dialog")}</Label>
        <Label muted>{t("Tab stays inside an open dialog. Held keys do not repeat application shortcuts.")}</Label>
      </>}
      {modal.kind === 'detach' && <Action testId="confirm-detach-profile" disabled={busy} primary onClick={() => {
        void model.detachProfile().then(done => { if (done) setModal(null); });
      }}>{t("Disconnect profile")}</Action>}
      {modal.kind === 'games' && <virtual-list estimatedItemHeight={64} style={{ height: 290 }}>
        {snapshot?.games.map(game => <div key={game.id} style={{ ...row, justifyContent: 'space-between', padding: 10 }}>
          <Label>{game.name}</Label><Action testId={`shop-game-${game.id}`} disabled={busy || !game.gamebanana} primary={state.shop.gameId === game.id}
            onClick={() => { model.setShopGame(game.id); setModal(null); }}>{game.gamebanana ? t("Browse") : t("Unavailable")}</Action>
        </div>)}
      </virtual-list>}
    </InfoDialog>}
    <ModDetailDialog features={features} />
    {(modal?.kind === 'link' || presentation.pendingLink) && <LinkDialog features={features} disabled={busy} close={() => setModal(null)} />}
    <ProtocolImportDialog runtime={managed} onLaunch={itemId => {
      managed.dismissProtocol();
      void features.openDetail(String(itemId));
    }} />
  </Palette.Provider>;
}
function Home({ features, snapshot, attach, navigate }: { features: DesktopFeatures; snapshot: Snapshot; attach: () => Promise<void>; navigate: (route: Route) => void }) {
  const t = useMessages();
  const selected = snapshot.installations.find(item => item.id === snapshot.selectedInstallationId);
  return <Page testId="home-ready" title={t("Your games, together.")} subtitle={t("A lighter native home for Deltamod Community.")}>
    <div style={{ ...column, flexGrow: 1, minHeight: 0, overflowY: 'scroll', paddingRight: 4 }}>
    <GlassPanel style={{ padding: 28, gap: 18 }}><Label size={22} bold>{snapshot.sourceAttached ? t("Your library is connected") : t("Bring your library into view")}</Label>
      <Label muted>{snapshot.sourceAttached ? t("This preview reads your existing profile without changing it.") : t("Choose your Deltamod data folder to see installations and mods. No copying, migration or game changes.")}</Label>
      <div style={row}><Action primary onClick={() => void attach()}>{t("Choose profile")}</Action><Action onClick={() => navigate('shop')}>{t("Explore Mod Shop")}</Action></div>
    </GlassPanel>
    <div style={{ ...row, alignItems: 'stretch' }}>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>{t("Installations")}</Label><Label size={34} bold>{snapshot.installations.length}</Label></GlassPanel>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>{t("Mods")}</Label><Label size={34} bold>{snapshot.mods.length}</Label></GlassPanel>
      <GlassPanel style={{ flexGrow: 1 }}><Label muted>{t("Theme palettes")}</Label><Label size={34} bold>{snapshot.themes.length}</Label></GlassPanel>
    </div>
    <ThemePreviewPanel features={features} themeId={snapshot.preferences.themeId} />
    {selected && <GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
      <div style={column}><Label muted>{t("Preview installation")}</Label><Label bold>{selected.name || selected.gameId}</Label></div>
      <Action onClick={() => navigate('installations')}>{t("Change selection")}</Action>
    </GlassPanel>}
    {snapshot.warnings.length > 0 && <GlassPanel><Label bold>{t("Some data needs attention")}</Label>{snapshot.warnings.map((warning, i) => <Label key={i} muted>{warning}</Label>)}</GlassPanel>}
    </div>
  </Page>;
}
function Library({ managed, model, snapshot, show, attach, searchRef }: { managed: ManagedRuntime; model: AppModel; snapshot: Snapshot; show: (mod: Mod) => void; attach: () => Promise<void>; searchRef: RefObject<PublicInstance | null> }) {
  const t = useMessages();
  const { library } = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const mods = useMemo(() => queryMods(snapshot.mods, library), [snapshot.mods, library]);
  const formats: LibraryQuery['format'][] = ['all', 'runtime', 'legacy packet'];
  const sorts: LibraryQuery['sort'][] = ['name-asc', 'name-desc', 'enabled-first'];
  return <Page title={t("Mod library")} subtitle={t("Profile-wide library. Installation selection does not change mod enabled states.")}>
    <ManagedLibraryPanel runtime={managed} />
    <input ref={searchRef} testId="library-search" value={library.query} placeholder={t("Search name, description or ID")} onChange={event => model.setLibrary({ query: event.value || '' })}
      style={{ height: 44, padding: 12, borderRadius: 12, backgroundColor: '#231c2b', color: colors.text, fontSize: 14 }} />
    <div style={{ ...column, gap: 8 }}>
      <div style={row}>{(['all', 'enabled', 'disabled', 'unknown'] as const).map(status =>
        <Action key={status} testId={`library-status-${status}`} primary={library.status === status} onClick={() => model.setLibrary({ status })}>
          {status === 'unknown' ? t("Unknown state") : status === 'all' ? t("All states") : status === 'enabled' ? t("Enabled") : t("Disabled")}
        </Action>)}</div>
      <div style={row}>
        <Action testId="library-format" onClick={() => model.setLibrary({ format: formats[(formats.indexOf(library.format) + 1) % formats.length]! })}>{t('Format: {format}', { format: t(`format.${library.format}`) })}</Action>
        <Action testId="library-sort" onClick={() => model.setLibrary({ sort: sorts[(sorts.indexOf(library.sort) + 1) % sorts.length]! })}>
          {library.sort === 'name-asc' ? t("Name A–Z") : library.sort === 'name-desc' ? t("Name Z–A") : t("Enabled first")}
        </Action>
        <Action testId="library-reset" onClick={() => model.setLibrary(DEFAULT_LIBRARY)}>{t("Reset")}</Action>
        <Label muted>{t('{visible} of {total}', { visible: mods.length, total: snapshot.mods.length })}</Label>
      </div>
    </div>
    {!snapshot.sourceAttached ? <><div testId="profile-detached"><Empty title={t("No profile attached")}>{t("Attach your Deltamod data folder to inspect its library.")}</Empty></div><Action onClick={() => void attach()}>{t("Choose profile")}</Action></>
      : mods.length === 0 ? <Empty title={t("No matching mods")}>{t("Try another search, or import mods in the current Tauri application.")}</Empty>
      : <virtual-list estimatedItemHeight={96} style={{ flexGrow: 1, minHeight: 0 }}>{mods.map((mod, i) => <div key={`${mod.format}:${mod.id}:${i}`} testId={`library-mod-${mod.id}`} style={{ paddingBottom: 10 }}>
        <GlassPanel style={{ ...row, justifyContent: 'space-between', padding: 16 }}><div style={{ ...column, gap: 7, flexGrow: 1, minWidth: 0 }}><Label bold>{mod.name}</Label>
          <div style={row}><Chip active={mod.enabled === true}>{mod.enabled === null ? t("State unavailable") : mod.enabled ? t("Enabled") : t("Disabled")}</Chip><Label muted size={12}>{t(`format.${mod.format}`)}</Label></div></div>
          <Action onClick={() => show(mod)}>{t("Details")}</Action>
        </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function Installations({ model, snapshot, show, attach }: { model: AppModel; snapshot: Snapshot; show: (installation: Installation) => void; attach: () => Promise<void> }) {
  const t = useMessages();
  const busy = model.state.loading || model.state.saving || model.state.shop.status === 'loading';
  return <Page title={t("Installations")} subtitle={t("Choose a preview context. Tauri's current installation and game files stay unchanged.")}>
    {snapshot.installations.length === 0 ? <><Empty title={t("No installations to display")}>{t("Choose a Deltamod data folder containing your configured installations.")}</Empty><Action onClick={() => void attach()}>{t("Choose profile")}</Action></>
      : <virtual-list estimatedItemHeight={100} style={{ flexGrow: 1, minHeight: 0 }}>{snapshot.installations.map(item => <div key={item.id} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
        <div style={column}><Label bold>{item.name || item.gameId}</Label><div style={row}><div testId={item.selected ? `preview-selected-${item.id}` : undefined}><Chip active={item.selected}>{item.selected ? t("Preview selection") : item.current ? t("Tauri current") : t("Installation")}</Chip></div><Label muted size={12}>{item.available === null ? t("Saved path · not checked") : item.available ? t("Folder found") : t("Folder missing")}</Label></div></div>
        <div style={row}><Action testId={`select-installation-${item.id}`} primary={item.selected} disabled={busy || item.selected}
          onClick={() => void model.selectInstallation(item.id)}>{item.selected ? t("Selected") : t("Select")}</Action>
          <Action testId={`browse-installation-${item.id}`} disabled={busy} onClick={() => {
            void model.selectInstallation(item.id).then(done => { if (done) model.navigate('shop'); });
          }}>{t("Browse mods")}</Action><Action onClick={() => show(item)}>{t("Details")}</Action></div>
      </GlassPanel></div>)}</virtual-list>}
  </Page>;
}
function ShopView({ features, model, searchRef, chooseGame }: { features: DesktopFeatures; model: AppModel; searchRef: RefObject<PublicInstance | null>; chooseGame: () => void }) {
  const t = useMessages();
  const { shop, snapshot, loading } = useSyncExternalStore(model.subscribe, model.getSnapshot);
  const [query, setQuery] = useState(shop.query);
  useEffect(() => { setQuery(shop.query); }, [shop.gameId, shop.query]);
  const game = snapshot?.games.find(item => item.id === shop.gameId);
  return <Page title={t("Find your next mod")} subtitle={t('{game} · GameBanana. Powered by the existing Rust network runtime.', { game: game?.name || t('Choose a game') })}>
    <div style={row}><Action testId="shop-game-picker" disabled={loading || shop.status === 'loading'} onClick={chooseGame}>{game?.name || t("Choose game")}</Action>
      <Label muted>{t("Library selection is session-only.")}</Label></div>
    {!shop.gameId && <Empty title={t("No Mod Shop mapping")}>{t("Choose a supported game from the catalogue. A missing mapping is not replaced with another game's results.")}</Empty>}
    <div style={row}><input ref={searchRef} testId="shop-search" value={query} placeholder={t("Search public mods")} onChange={event => setQuery(event.value || '')} onSubmit={() => { if (shop.status !== 'loading') void model.browse(query); }}
      style={{ height: 44, padding: 12, flexGrow: 1, color: colors.text, fontSize: 14, backgroundColor: '#231c2b', borderRadius: 12 }} />
      <Action primary disabled={loading || !shop.gameId || shop.status === 'loading'} onClick={() => void model.browse(query)}>{t("Search")}</Action></div>
    {shop.status === 'loading' && <Label muted>{t("Loading GameBanana...")}</Label>}
    {shop.status === 'error' && <GlassPanel><Label>{shop.error}</Label><Action onClick={() => void model.browse(shop.query, shop.page)}>{t("Retry")}</Action></GlassPanel>}
    {shop.status === 'ready' && shop.items.length === 0 && <Empty title={t("No results")}>{t("Try a different search.")}</Empty>}
    <virtual-list estimatedItemHeight={108} style={{ flexGrow: 1, minHeight: 0 }}>{shop.items.map((item, i) => <div key={`${item.id}:${i}`} style={{ paddingBottom: 10 }}><GlassPanel style={{ ...row, justifyContent: 'space-between' }}>
      <div style={{ ...column, gap: 6, minWidth: 0, flexGrow: 1 }}><Label bold>{item.name}</Label><Label muted size={12}>{item.author || t("GameBanana")}</Label></div>
      <Action testId={`shop-detail-${item.id}`} onClick={() => void features.openDetail(item.id)}>{t("View mod")}</Action>
    </GlassPanel></div>)}</virtual-list>
    <div style={{ ...row, justifyContent: 'space-between' }}><Label muted>{t('Page {page} · Downloads and account login remain in Tauri.', { page: shop.page })}</Label><div style={row}>
      <Action disabled={loading || shop.status !== 'ready' || shop.page <= 1} onClick={() => void model.browse(shop.query, shop.page - 1)}>{t("Previous")}</Action>
      <Action disabled={loading || shop.status !== 'ready' || !shop.hasMore || shop.page >= 100} onClick={() => void model.browse(shop.query, shop.page + 1)}>{t("Next")}</Action>
    </div></div>
  </Page>;
}
