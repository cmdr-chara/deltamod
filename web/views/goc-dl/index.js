/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    const progress = document.getElementById('up');
    const output = document.getElementById('download-percent');
    let frame = null;
    let latest = null;
    window.currentPageStack.onDLP = percentage => {
        latest = percentage;
        if (frame !== null) return;
        frame = requestAnimationFrame(() => {
            frame = null;
            if (!progress.isConnected) return;
            const amount = window.DeltamodUI.setProgress(progress, latest);
            output.textContent = amount === null ? '' : `${Math.round(amount)}%`;
        });
    };
    window.DeltamodUI.onDispose(() => { if (frame !== null) cancelAnimationFrame(frame); });
})();
