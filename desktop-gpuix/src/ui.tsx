// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createContext, useContext, type ReactNode } from 'react';
import { motion, Button, Dialog, DialogPortal, DialogBackdrop, DialogPopup, DialogTitle, DialogDescription, DialogClose } from '@gpuix/react';
import type { StyleDesc } from '@gpuix/react';
import type { Preferences } from './contracts.js';
import { useMessages } from './i18n.js';

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
  return <Button testId={testId} disabled={disabled} onClick={onClick} style={{
    ...row, justifyContent: 'center', padding: 10, paddingLeft: 16, paddingRight: 16,
    borderRadius: 12, borderWidth: 1, borderColor: primary ? theme.accent : colors.edge,
    backgroundColor: primary ? theme.accent : '#24202b', opacity: disabled ? 0.45 : 1,
    hover: { backgroundColor: primary ? theme.accent : '#352d3d' },
  }}><Label bold ink={primary ? accentInk(theme.accent) : colors.text}>{children}</Label></Button>;
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
    <div style={{ ...column, gap: 6 }}><Label size={29} bold>{title}</Label><Label muted>{subtitle}</Label></div>
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
export function InfoDialog({ title, description, children, close }: { title: string; description: string; children?: ReactNode; close: () => void }) {
  const t = useMessages();
  return <Dialog open onOpenChange={open => { if (!open) close(); }}>
    <DialogPortal>
      <DialogBackdrop style={{ backgroundColor: '#000000a0' }} />
      <DialogPopup style={{ ...column, width: 500, maxWidth: '90%', padding: 24, gap: 18, backgroundColor: '#211a29', borderWidth: 1, borderColor: colors.edge, borderRadius: 20 }}>
        <DialogTitle style={{ fontSize: 21, color: colors.text }}>{title}</DialogTitle>
        <DialogDescription style={{ fontSize: 14, color: colors.muted }}>{description}</DialogDescription>
        {children}
        <DialogClose style={{ padding: 12, borderRadius: 10, backgroundColor: '#3a3046' }}><Label bold>{t("Close")}</Label></DialogClose>
      </DialogPopup>
    </DialogPortal>
  </Dialog>;
}
