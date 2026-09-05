/* Load each renderer dependency once; an optional boot bundle must not strand the route guard. */
(() => {
    'use strict';
    const load = source => new Promise((resolve, reject) => {
        const script = document.createElement('script');
        script.src = source; script.async = false;
        script.onload = resolve;
        script.onerror = () => reject(new Error(`Unable to load ${source}`));
        document.body.appendChild(script);
    });
    const reveal = () => {
        document.documentElement.classList.remove('deltamod-route-pending');
        window.dispatchEvent(new Event('deltamod-route-ready'));
    };
    const fail = error => {
        document.getElementById('deltamod-boot-root').hidden = true;
        document.body.classList.remove('deltamod-boot-active');
        const viewport = document.querySelector('.viewport');
        if (window.DeltamodUI) window.DeltamodUI.showError(viewport, error, () => location.reload());
        else {
            const message = document.createElement('p');
            message.textContent = 'Deltamod could not start. Close and reopen the application.';
            viewport.replaceChildren(message);
        }
        reveal();
    };
    async function start() {
        // Installer mode is native routing; never start the Community renderer twice on failure.
        const installer = await window.deltamodBackend?.invoke('isInstallerMode', []).catch(() => false);
        if (installer === true) { location.replace('./installer/index.html'); return; }
        for (const source of ['./modules/localization.js', './modules/icons.js', './modules/workspace.js', './modules/dialogs.js', './modules/theme-sprites.js', './modules/seasonal-events.js']) await load(source);
        try { await load('./boot/deltamod-boot.js'); }
        catch (_) { document.getElementById('deltamod-boot-root').hidden = true; }
        await load('index.js');
        for (const source of ['./linux-menu-audio.js', './linux-runtime-polish.js']) {
            try { await load(source); } catch (error) { console.warn(error); }
        }
        reveal();
    }
    start().catch(fail);
})();
