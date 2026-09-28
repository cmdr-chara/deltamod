// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
(function (root, factory) {
    const mount = factory();
    if (typeof module !== 'undefined' && module.exports) module.exports = mount;
    if (root?.document && root.communityAPI?.updates) {
        const start = () => {
            if (!root.deltamodUpdateNotice) root.deltamodUpdateNotice = mount(root.document, root.communityAPI.updates);
        };
        if (root.document.readyState === 'loading') root.document.addEventListener('DOMContentLoaded', start, { once: true });
        else start();
    }
})(typeof window === 'undefined' ? null : window, () => {
    'use strict';
    return function mount(document, api) {
        const panel = document.createElement('aside');
        panel.className = 'community-update-notice';
        panel.setAttribute('aria-label', 'Application update');
        panel.hidden = true;
        const title = document.createElement('strong');
        title.textContent = 'Deltamod update';
        const message = document.createElement('p');
        message.setAttribute('role', 'status');
        const progress = document.createElement('progress');
        progress.setAttribute('aria-label', 'Update download');
        const cancel = document.createElement('button');
        cancel.type = 'button'; cancel.textContent = 'Cancel download';
        const dismiss = document.createElement('button');
        dismiss.type = 'button'; dismiss.textContent = 'Dismiss'; dismiss.hidden = true;
        panel.append(title, message, progress, cancel, dismiss);
        document.body.append(panel);
        let phase = 'idle';
        let disposed = false;
        let generation = 0;
        const active = new Set(['downloading', 'downloaded', 'cancelling', 'installing']);
        const descriptions = {
            downloading: 'Downloading the signed update.',
            downloaded: 'Publisher verified. Preparing installation.',
            cancelling: 'Cancelling the download...',
            installing: 'Installing the update. Do not close Deltamod.',
            installed: 'Update installed. Close and reopen Deltamod to use the new version.',
            cancelled: 'Download cancelled. Your installed version has not changed.',
            failed: 'The update failed. Your current installation has not been replaced by unverified data.'
        };
        const status = data => {
            if (disposed || !data || typeof data.state !== 'string' || !descriptions[data.state]) return;
            if (data.state === 'downloading' && phase !== 'downloading') {
                generation += 1;
                progress.removeAttribute('value');
            }
            phase = data.state;
            panel.hidden = false;
            message.textContent = phase === 'failed' && typeof data.reason === 'string'
                ? data.reason.slice(0, 512) : descriptions[phase];
            cancel.hidden = !active.has(phase);
            cancel.disabled = phase !== 'downloading';
            progress.hidden = !['downloading', 'cancelling'].includes(phase);
            dismiss.hidden = active.has(phase);
            cancel.textContent = phase === 'cancelling' ? 'Cancelling...' : 'Cancel download';
        };
        const onProgress = data => {
            if (disposed || phase !== 'downloading' || data?.operationId !== 'community-update') return;
            if (Number.isSafeInteger(data.total) && data.total > 0 && Number.isSafeInteger(data.completed) && data.completed >= 0) {
                progress.max = data.total;
                progress.value = Math.min(data.completed, data.total);
            } else progress.removeAttribute('value');
        };
        const onCancel = async () => {
            if (disposed || phase !== 'downloading' || cancel.disabled) return;
            const request = generation;
            cancel.disabled = true;
            try {
                const accepted = await api.cancel();
                if (disposed || generation !== request || phase !== 'downloading') return;
                if (accepted) status({ state: 'cancelling' });
                else {
                    const current = await api.status();
                    if (!disposed && generation === request) status(current);
                }
            } catch {
                if (!disposed && generation === request && phase === 'downloading') {
                    cancel.disabled = false;
                    message.textContent = 'Could not cancel the download. Try again.';
                }
            }
        };
        const onDismiss = () => { if (!active.has(phase)) panel.hidden = true; };
        cancel.addEventListener('click', onCancel);
        dismiss.addEventListener('click', onDismiss);
        const subscriptions = [api.onStatus(status), api.onProgress(onProgress)];
        return Object.freeze({
            failure(reason) { if (phase !== 'cancelled' && phase !== 'failed') status({ state: 'failed', reason }); },
            dispose() {
                if (disposed) return;
                disposed = true;
                generation += 1;
                subscriptions.forEach(off => { if (typeof off === 'function') off(); });
                cancel.removeEventListener('click', onCancel);
                dismiss.removeEventListener('click', onDismiss);
                panel.remove();
            }
        });
    };
});
