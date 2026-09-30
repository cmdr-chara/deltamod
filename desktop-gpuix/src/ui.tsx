// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createContext, useContext, useRef, useLayoutEffect, type ReactNode } from 'react';
import { motion, useGpuixRequired, type PublicInstance } from '@gpuix/react';
import type { StyleDesc } from '@gpuix/react';
import type { Preferences } from './contracts.js';
import { useMessages } from './i18n.js';
import { registerDialog, dialogCount } from './modal-state.mjs';
import { restoreDialogFocus } from './window-lifecycle.mjs';

export const Palette = createContext<Preferences>({ themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true });
export const colors = { text: '#f3eef0', muted: '#b9adb5', faint: '#867b87', surface: '#17131d', edge: '#ffffff22', success: '#9fe0ba' };
export const row: StyleDesc = { display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 12 };
export const column: StyleDesc = { display: 'flex', flexDirection: 'column', gap: 12 };
export function Label({ children, muted = false, size = 14, bold = false, ink }: { children: ReactNode; muted?: boolean; size?: number; bold?: boolean; ink?: string }) {
  return <text style={{ color: ink || (muted ? colors.muted : colors.text), fontSize: size, fontWeight: bold ? 700 : 400 }}>{children}</text>;
}
function accentInk(accent: string) {
  const rgb = [1, 3, 5].map(index => parseInt(accent.slice(index, index + 2), 16) / 255)
    .map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
  const luminance = 0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!;
  return luminance > 0.179 ? '#000000' : '#ffffff';
}
export function Action({ children, onClick, disabled = false, primary = false, testId }: {
  children: ReactNode; onClick: () => void; disabled?: boolean; primary?: boolean; testId?: string;
}) {
  const theme = useContext(Palette);
  const t = useMessages();
  const activate = () => { if (!disabled) onClick(); };
  return <div testId={testId} role="button" aria-label={typeof children === 'string' ? children : undefined}
    aria-description={disabled ? t('Unavailable') : undefined} tabIndex={disabled ? -1 : 0}
    onClick={disabled ? undefined : activate} onKeyDown={disabled ? undefined : event => { if (event.key === 'enter' || event.key === 'space') activate(); }}
    style={{
      ...row, justifyContent: 'center', padding: 10, paddingLeft: 16, paddingRight: 16,
      borderRadius: 12, borderWidth: 1, borderColor: primary ? theme.accent : colors.edge,
      backgroundColor: primary ? theme.accent : '#24202b', opacity: disabled ? 0.45 : 1,
      cursor: disabled ? 'default' : 'pointer', userSelect: 'none',
      hover: disabled ? {} : { backgroundColor: primary ? theme.accent : '#352d3d' },
      active: disabled ? {} : { opacity: 0.82 },
    }}><Label bold ink={primary ? accentInk(theme.accent) : colors.text}>{children}</Label></div>;
}
export function GlassPanel({ children, style = {}, testId }: { children: ReactNode; style?: StyleDesc; testId?: string }) {
  const theme = useContext(Palette);
  return <div testId={testId} style={{ ...column, padding: 20, borderRadius: 20,
    borderWidth: 1, borderColor: colors.edge, backgroundColor: theme.opaque ? colors.surface : '#211b2bd9', ...style }}>
    {children}
  </div>;
}
export function Page({ children, title, subtitle, testId }: { children: ReactNode; title: string; subtitle: string; testId?: string }) {
  const theme = useContext(Palette);
  return <motion.div testId={testId} initial={theme.reducedMotion ? false : { opacity: 0 }} animate={{ opacity: 1 }}
    transition={{ duration: theme.reducedMotion ? 0 : 0.16, ease: 'easeOut' }}
    style={{ ...column, height: '100%', minHeight: 0, flexGrow: 1, padding: 28, gap: 20 }}>
    <div style={{ ...column, gap: 6 }}><text role="heading" aria-level={1} style={{fontSize:29,fontWeight:700,color:colors.text}}>{title}</text><Label muted>{subtitle}</Label></div>
    {children}
  </motion.div>;
}
export function Empty({ title, children }: { title: string; children: ReactNode }) {
  return <GlassPanel style={{ padding: 28 }}><Label size={19} bold>{title}</Label><Label muted>{children}</Label></GlassPanel>;
}
export function Chip({ children, active = false }: { children: ReactNode; active?: boolean }) {
  const theme = useContext(Palette);
  return <div style={{ padding: 5, paddingLeft: 9, paddingRight: 9, borderRadius: 8, backgroundColor: active ? `${theme.accent}33` : '#ffffff0d' }}>
    <text style={{ fontSize: 12, color: active ? colors.text : colors.muted }}>{children}</text>
  </div>;
}
export function InfoDialog({ title, description, children, close, closeDisabled = false }: { closeDisabled?: boolean; title: string; description: string; children?: ReactNode; close: () => void }) {
  const t = useMessages();
  const renderer = useGpuixRequired();
  const popup = useRef<PublicInstance | null>(null);
  // Capture the opener before this dialog's autoFocus is committed.
  const previous = useRef<number | null | undefined>(undefined);
  if (previous.current === undefined) previous.current = renderer.getFocusedElementId?.() ?? null;
  useLayoutEffect(() => {
    const unregister = registerDialog();
    return () => {
      unregister();
      // A replacement dialog owns focus. Do not restore into its background.
      if (dialogCount() === 0) restoreDialogFocus(renderer, previous.current, popup.current?.id);
    };
  }, [renderer]);
  const dismiss = () => { if (!closeDisabled) close(); };
  return <div role="presentation" style={{
    position: 'absolute', top: 0, right: 0, bottom: 0, left: 0,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    backgroundColor: '#000000a0', pointerEvents: 'auto',
  }}>
    <div ref={popup} role="dialog" aria-label={title} aria-description={description} autoFocus tabIndex={0}
      onMouseDownOutside={dismiss}
      onKeyDown={event => {
        if (event.key === 'escape') dismiss();
        if (event.key === 'tab' && popup.current) {
          if (event.modifiers?.shift) renderer.focusPreviousWithin?.(popup.current.id);
          else renderer.focusNextWithin?.(popup.current.id);
        }
      }}
      style={{ ...column, width: 500, maxWidth: '90%', maxHeight: '86%', overflowY: 'scroll', padding: 24, gap: 18,
        backgroundColor: '#211a29', borderWidth: 1, borderColor: colors.edge, borderRadius: 20, pointerEvents: 'auto' }}>
      <text role="heading" aria-level={2} style={{ fontSize: 21, fontWeight: 700, color: colors.text }}>{title}</text>
      <text style={{ fontSize: 14, color: colors.muted }}>{description}</text>
      {children}
      <Action disabled={closeDisabled} onClick={dismiss}>{t("Close")}</Action>
    </div>
  </div>;
}
