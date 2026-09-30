// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useEffect, useState, useSyncExternalStore } from 'react';
import type { DesktopFeatures } from './features.mjs';
import { openModPage } from './options.mjs';
import { useMessages, useByteFormat } from './i18n.js';
import { Action, GlassPanel, InfoDialog, Label, row, column, colors } from './ui.js';

export function ThemePreviewPanel({ features, themeId }: { features: DesktopFeatures; themeId: string }) {
  const state = useSyncExternalStore(features.subscribe, features.getSnapshot);
  const t = useMessages();
  useEffect(() => { if (state.preferences.themeImages) void features.loadTheme(themeId); }, [features, themeId, state.preferences.themeImages]);
  if (!state.preferences.themeImages) return null;
  return <GlassPanel testId="theme-preview" style={{ padding: 12, gap: 8, flexShrink: 0 }}>
    <Label bold>{t('Theme preview')}</Label>
    {state.theme.status === 'loading' && <Label muted>{t('Loading theme image...')}</Label>}
    {state.theme.status === 'error' && <><Label muted>{t('The theme image could not be loaded.')}</Label>
      <Action onClick={() => void features.retryTheme()}>{t('Retry')}</Action></>}
    {state.theme.value?.imagePath ? <img src={state.theme.value.imagePath} alt={t('Built-in theme background')} objectFit="cover"
      style={{ width: '100%', height: 130, borderRadius: 12 }} />
      : state.theme.status === 'ready' && <Label muted>{t('This theme has no still background image.')}</Label>}
    {(state.theme.value?.hasVideo || state.theme.value?.hasAudio) && <Label muted size={12}>
      {t('Video and audio remain in Tauri. This preview displays the still image only.')}
    </Label>}
  </GlassPanel>;
}

export function PresentationSettings({ features, disabled, openLink }: { features: DesktopFeatures; disabled: boolean; openLink: () => void }) {
  const state = useSyncExternalStore(features.subscribe, features.getSnapshot);
  const t = useMessages();
  return <GlassPanel>
    <Label size={18} bold>{t('Interface')}</Label>
    <div style={{ ...row, justifyContent: 'space-between' }}>
      <Label>{t('Language')}</Label>
      <div style={row}>{(['en', 'it'] as const).map(locale => <Action key={locale} testId={`locale-${locale}`}
        disabled={disabled || state.saving} primary={state.preferences.locale === locale}
        onClick={() => void features.savePreferences({ locale })}>{locale === 'en' ? 'English' : 'Italiano'}</Action>)}</div>
    </div>
    <div style={{ ...row, justifyContent: 'space-between' }}><Label>{t('Theme images')}</Label>
      <Action testId="toggle-theme-images" disabled={disabled || state.saving}
        onClick={() => void features.savePreferences({ themeImages: !state.preferences.themeImages })}>
        {t(state.preferences.themeImages ? 'On' : 'Off')}
      </Action>
    </div>
    <Label muted size={12}>{t('Images are off by default. Only the selected built-in image is requested.')}</Label>
    <Action testId="open-preview-link" disabled={disabled || state.saving} onClick={openLink}>{t('Open a link')}</Action>
  </GlassPanel>;
}

export function ModDetailDialog({ features }: { features: DesktopFeatures }) {
  const state = useSyncExternalStore(features.subscribe, features.getSnapshot);
  const [openerError, setOpenerError] = useState('');
  const t = useMessages();
  const bytes = useByteFormat();
  const detail = state.detail;
  useEffect(() => { setOpenerError(''); }, [detail.id]);
  if (detail.status === 'idle') return null;
  const value = detail.value;
  return <InfoDialog close={() => features.closeDetail()} title={value?.name || t('Mod details')}
    description={t('Public GameBanana information. Opening details does not download or install anything.')}>
    {detail.status === 'loading' && <Label>{t('Loading mod details...')}</Label>}
    {detail.status === 'error' && <><Label>{t('The mod details could not be loaded.')}</Label>
      <Label muted size={12}>{detail.error}</Label>
      <Action testId="retry-mod-detail" onClick={() => void features.openDetail(detail.id)}>{t('Retry')}</Action></>}
    {value && <>
      <div style={{ ...column, height: 310, overflowY: 'scroll' }}>
        {value.author && <Label muted>{t('By {author}', { author: value.author })}</Label>}
        {value.game && <Label muted>{t('Game: {game}', { game: value.game })}</Label>}
        {value.hasContentRatings && <Label muted>{t('This mod has content ratings. Review them on its GameBanana page.')}</Label>}
        <Label>{value.description || t('No description is available.')}</Label>
        <Label bold>{t('Available files')}</Label>
        {value.files.length === 0 && <Label muted>{t('No file information is available.')}</Label>}
        {value.files.map(file => <div key={file.id} style={{ ...column, gap: 4, padding: 8, borderRadius: 8, backgroundColor: '#ffffff09' }}>
          <Label>{file.name || t('Unnamed file')}</Label>
          <Label muted size={12}>{t('Version: {version} · {size}', { version: file.version || t('Not specified'), size: bytes(file.bytes) })}</Label>
        </div>)}
      </div>
      <Label muted size={12}>{t('File metadata is not a security scan. Downloads and installation remain in Tauri.')}</Label>
      <Action testId="open-mod-browser" primary onClick={() => {
        setOpenerError('');
        void openModPage(value.url).catch(() => setOpenerError(t('The browser could not be opened.')));
      }}>{t('Open GameBanana page')}</Action>
    </>}
    {openerError && <Label>{openerError}</Label>}
  </InfoDialog>;
}

export function LinkDialog({ features, close, disabled }: { features: DesktopFeatures; close: () => void; disabled: boolean }) {
  const state = useSyncExternalStore(features.subscribe, features.getSnapshot);
  const [raw, setRaw] = useState('');
  const t = useMessages();
  const intent = state.pendingLink;
  const dismiss = () => { features.dismissLink(); close(); };
  return <InfoDialog close={dismiss} title={t('Open a link')}
    description={t('Preview links only navigate or show public mod details. They cannot change game files.')}>
    {!intent && <><input testId="preview-link-input" value={raw} placeholder={t('Paste a GameBanana mod URL or preview link')}
      onChange={event => setRaw(event.value || '')}
      onSubmit={() => features.reviewLink(raw.trim())}
      style={{ height: 44, padding: 12, fontSize: 14, color: colors.text, backgroundColor: '#30263a', borderRadius: 10 }} />
      <Action testId="review-preview-link" disabled={disabled} onClick={() => features.reviewLink(raw.trim())}>{t('Review link')}</Action></>}
    {state.error && <Label>{t(state.error)}</Label>}
    {intent && <>
      <Label>{intent.kind === 'mod' ? t('View public mod {id}', { id: intent.id }) : intent.kind === 'screen'
        ? t('Navigate to {screen}', { screen: t(`screen.${intent.route}`) })
        : t('Browse {game} for "{query}"', { game: intent.gameId, query: intent.query })}</Label>
      <Action testId="confirm-preview-link" primary disabled={disabled} onClick={() => { if (features.applyLink()) close(); }}>{t('Continue')}</Action>
      <Action disabled={disabled} onClick={() => features.dismissLink()}>{t('Edit link')}</Action>
    </>}
  </InfoDialog>;
}
