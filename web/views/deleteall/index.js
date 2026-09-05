/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    'use strict';
    const button = document.getElementById('initbtn');
    const status = document.getElementById('reset-status');
    const progress = document.getElementById('progbar');
    const available = window.deltamodBackend.isCommandAvailable('initialize');
    let running = false;
    button.disabled = !available;
    if (!available) button.title = 'Data reset is unavailable in this app build';
    window.currentPageStack.init = async () => {
        if (!available || running) return;
        running = true; button.disabled = true;
        try {
            const confirmed = await htmlAlert('Reset Deltamod data',
                'This permanently removes your Deltamod settings and managed data. This action cannot be undone. Continue only after making a backup.', [
                    { text: 'Reset data', resolveWith: 'reset' }, { text: 'Cancel', resolveWith: false }
                ], 'warning');
            if (confirmed !== 'reset' || !button.isConnected) return;
            progress.hidden = false; progress.removeAttribute('value');
            status.textContent = 'Resetting Deltamod data…';
            await window.deltamodBackend.invokeOptional('initialize', [], false);
            status.textContent = 'Reset request completed.';
            progress.value = 100;
        } catch (error) {
            status.textContent = String(error?.message || error);
            progress.hidden = true;
        } finally { running = false; button.disabled = !available; }
    };
    button.addEventListener('click', window.currentPageStack.init);
})();
