// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
(function (scope, factory) {
    if (typeof module === 'object' && module.exports) module.exports = factory;
    else factory(scope);
})(typeof window === 'undefined' ? globalThis : window, function installDesktopStartup(scope) {
    'use strict';
    if (scope.DeltamodStartup) return scope.DeltamodStartup;
    const doc = scope.document;
    const clock = () => scope.performance.now();
    const spans = [];
    const marks = { loader: clock() };
    const pending = new Map();
    const known = new Set([
        './modules/localization.js', './modules/frontend-refinements.js',
        './modules/theme-sprites.js', './modules/seasonal-events.js',
        './boot/deltamod-boot.js', 'index.js', './media-compat.js',
        './linux-menu-audio.js', './linux-runtime-polish.js', './modules/updater-notice.js'
    ]);
    let starting = null;
    let sealed = false;
    let traceSink = null;
    let lazyTimer = null;
    let stopped = false;
    let sealTimer = null;

    function record(kind, name, start, duration, ok = true) {
        if (sealed || spans.length >= 96 || !Number.isFinite(duration) || duration < 0) return;
        // Only operation names and timing: never payloads, URLs, tokens or paths.
        spans.push({ kind, name: String(name).slice(0, 64), start: Math.round(start), ms: Math.round(duration), ok });
    }

    function load(source) {
        if (!known.has(source)) return Promise.reject(new Error('Unknown startup script'));
        if (pending.has(source)) return pending.get(source);
        const result = new Promise((resolve, reject) => {
            const script = doc.createElement('script');
            const start = clock();
            // Fetches overlap; classic scripts still execute in insertion order.
            script.async = false;
            script.src = source;
            script.onload = () => { record('script', source, start, clock() - start); resolve(); };
            script.onerror = () => { record('script', source, start, clock() - start, false); reject(new Error(`Unable to load ${source}`)); };
            doc.body.appendChild(script);
        });
        pending.set(source, result);
        return result;
    }

    function snapshot() {
        return { schemaVersion: 1, marks: { ...marks }, spans: spans.map(span => ({ ...span })) };
    }

    function publish(phase) {
        if (!traceSink) return;
        // The backend logger accepts at most 2048 characters per message.
        const slow = [...spans].sort((a, b) => b.ms - a.ms).slice(0, 12);
        const payload = JSON.stringify({ phase, marks, slow });
        if (payload.length <= 1900) Promise.resolve(traceSink(`DELTAMOD_STARTUP ${payload}`)).catch(() => {});
    }

    function benchmarkReady(sink) {
        if (sealed) return;
        marks.rendererReady = clock();
        traceSink = sink;
        sealed = true;
        publish('renderer-ready');
    }

    function onBootDismissed() {
        marks.bootDismissed = clock();
        publish('boot-dismissed');
    }
    scope.addEventListener('deltamod-boot-dismissed', onBootDismissed, { once: true });

    function stop() {
        stopped = true;
        sealed = true;
        if (sealTimer !== null) scope.clearTimeout(sealTimer);
        sealTimer = null;
        if (lazyTimer !== null) {
            if (scope.cancelIdleCallback) scope.cancelIdleCallback(lazyTimer);
            else scope.clearTimeout(lazyTimer);
        }
        scope.removeEventListener('deltamod-boot-dismissed', onBootDismissed);
        traceSink = null;
        pending.clear();
    }
    scope.addEventListener('pagehide', stop, { once: true });

    function start() {
        if (starting) return starting;
        starting = (async () => {
            const installer = await (scope.deltamodBackend?.invoke
                ? scope.deltamodBackend.invoke('isInstallerMode', []) : false);
            if (stopped) return;
            if (installer === true) {
                scope.location.replace('./installer/index.html');
                return;
            }
            const linux = /linux/i.test(`${scope.navigator?.platform || ''} ${scope.navigator?.userAgent || ''}`);
            const core = [
                ...(linux ? ['./media-compat.js'] : []),
                './modules/localization.js', './modules/frontend-refinements.js',
                './modules/theme-sprites.js', './modules/seasonal-events.js',
                './boot/deltamod-boot.js',
                // The guarded loader appends the normal renderer (src="index.js").
                'index.js',
                ...(linux ? ['./linux-menu-audio.js', './linux-runtime-polish.js'] : [])
            ];
            await Promise.all(core.map(load));
            if (stopped) return;
            marks.scriptsReady = clock();
            doc.documentElement.classList.remove('deltamod-route-pending');
            scope.dispatchEvent(new scope.Event('deltamod-route-ready'));
            const lazy = () => {
                lazyTimer = null;
                if (!stopped) load('./modules/updater-notice.js').catch(error => scope.console.warn(error.message));
            };
            // The updater notice queries native status on installation, so it can
            // recover events emitted before this noncritical UI module is loaded.
            lazyTimer = scope.requestIdleCallback ? scope.requestIdleCallback(lazy, { timeout: 1500 }) : scope.setTimeout(lazy, 0);
            sealTimer = scope.setTimeout(() => { sealed = true; sealTimer = null; }, 30000);
        })().catch(error => {
            // Do not retry the whole renderer after partial initialization.
            scope.console.error('Deltamod startup failed:', error);
            doc.documentElement.classList.remove('deltamod-route-pending');
            scope.DeltamodBoot?.fail('Unable to finish startup');
            const host = doc.querySelector('.viewport');
            if (host) host.textContent = 'Deltamod could not start. Close and reopen the application.';
            throw error;
        });
        return starting;
    }
    const api = Object.freeze({ start, load, record, snapshot, benchmarkReady });
    scope.DeltamodStartup = api;
    return api;
});
