/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    'use strict';
    const ui = window.DeltamodUI;
    const log = document.getElementById('gpl');
    const progress = document.getElementById('patch-progress');
    const value = document.getElementById('patch-percent');
    const title = document.getElementById('patchingTXT');
    const nextButton = document.getElementById('next');
    const hasLegacyNextPatchStep = window.deltamodBackend.isCommandAvailable('npsCallback');
    const lines = [];
    let frame = null;
    let nextRunning = false;
    let completed = false;
    let latestPercent = null;
    if (!hasLegacyNextPatchStep) {
        nextButton.textContent = ui.t('ui_return_mods', 'Return to mods');
        nextButton.title = nextButton.textContent;
    }
    function flush() {
        frame = null;
        if (!log.isConnected) return;
        const followTail = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
        const fragment = document.createDocumentFragment();
        for (const message of lines.splice(0)) {
            const line = document.createElement('div');
            line.textContent = message; fragment.append(line);
        }
        log.appendChild(fragment);
        while (log.childElementCount > 200) log.firstElementChild.remove();
        if (followTail) log.scrollTop = log.scrollHeight;
        ui.setProgress(progress, latestPercent);
        value.textContent = latestPercent === null ? '' : `${Math.round(latestPercent)}%`;
    }
    window.currentPageStack.gpl = event => {
        if (!log.isConnected || completed) return;
        const message = String(event?.log ?? '');
        if (message) lines.push(message.slice(0, 4000));
        if (lines.length > 200) lines.splice(0, lines.length - 200);
        // Native events without a measurable percentage remain indeterminate.
        if (event?.percent !== undefined) latestPercent = ui.percent(event.percent);
        if (frame === null) frame = requestAnimationFrame(flush);
    };
    window.currentPageStack.next = async () => {
        if (nextRunning) return;
        nextRunning = true;
        try {
            if (hasLegacyNextPatchStep) {
                await window.deltamodBackend.invokeOptional('npsCallback', [], false);
                return;
            }
            await page('main');
        } catch (error) {
            await htmlAlert('Unable to continue', String(error?.message || error), [{ text: 'OK' }]);
        } finally { nextRunning = false; }
    };
    window.currentPageStack.fp = () => {
        completed = true; latestPercent = 100;
        if (frame !== null) cancelAnimationFrame(frame);
        flush();
        title.removeAttribute('data-i18n');
        title.textContent = ui.t('ui_patch_complete', 'Patching complete');
        title.classList.add('success');
        document.getElementById('patchingDesc').textContent = ui.t('ui_patch_ready', 'Your selected mods have been applied.');
        nextButton.hidden = false;
    };
    ui.onDispose(() => { if (frame !== null) cancelAnimationFrame(frame); lines.length = 0; });
})();
