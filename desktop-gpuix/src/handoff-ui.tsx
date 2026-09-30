// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { useSyncExternalStore, type ComponentProps } from 'react';
import { App } from './app.js';
import { LanguageContext, useMessages } from './i18n.js';
import type { HandoffInbox } from './handoffs.mjs';
import { subscribeDialogs, dialogCount } from './modal-state.mjs';
import { Action, Label, Palette, column, row } from './ui.js';

type Props = ComponentProps<typeof App> & { inbox: HandoffInbox };
export function HandoffShell({ inbox, ...props }: Props) {
  const state = useSyncExternalStore(inbox.subscribe, inbox.getSnapshot);
  const presentation = useSyncExternalStore(props.features.subscribe, props.features.getSnapshot);
  const model = useSyncExternalStore(props.model.subscribe, props.model.getSnapshot);
  const managed = useSyncExternalStore(props.managed.subscribe, props.managed.getSnapshot);
  const dialogs = useSyncExternalStore(subscribeDialogs, dialogCount);
  const disabled = dialogs > 0 || state.reviewing || model.loading || model.saving || presentation.saving
    || managed.loading || !!managed.busy || managed.protocol.status !== 'idle'
    || presentation.detail.status !== 'idle' || !!presentation.pendingLink;
  const preferences = model.snapshot?.preferences;
  return <LanguageContext.Provider value={presentation.preferences.locale}>
    <div style={{ ...column, width: '100%', height: '100%', gap: 0, position: 'relative', backgroundColor: '#0e0b13' }}>
      {state.items.length > 0 && <Palette.Provider value={preferences ?? { themeId: 'base', accent: '#cd4451', reducedMotion: true, opaque: true }}>
        <HandoffBar inbox={inbox} disabled={disabled} props={props} />
      </Palette.Provider>}
      <div style={{ flexGrow: 1, minHeight: 0, position: 'relative' }}><App {...props} /></div>
    </div>
  </LanguageContext.Provider>;
}
function HandoffBar({ inbox, disabled, props }: { inbox: HandoffInbox; disabled: boolean; props: ComponentProps<typeof App> }) {
  const state = useSyncExternalStore(inbox.subscribe, inbox.getSnapshot);
  const t = useMessages();
  const first = state.items[0];
  if (!first) return null;
  return <div testId="handoff-inbox" role="region" aria-label="Desktop open requests"
    style={{ ...column, gap: 6, padding: 12, flexShrink: 0, backgroundColor: '#24192c' }}>
    <div style={{ ...row, justifyContent: 'space-between' }}>
      <Label>{String(state.items.length)} {t('Desktop open requests waiting for review')}</Label>
      <div style={row}>
        <Action testId="review-handoff" disabled={disabled} onClick={() => void inbox.review(props.features, props.managed, props.model)}>{t('Review request')}</Action>
        <Action testId="discard-handoff" disabled={disabled} onClick={() => inbox.discard(first.id)}>{t('Dismiss')}</Action>
      </div>
    </div>
    {state.error && <div role="alert"><Label>{state.error}</Label></div>}
  </div>;
}
