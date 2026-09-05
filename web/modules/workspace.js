/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(function (root, factory) {
    const api = factory(root);
    if (typeof module === 'object' && module.exports) module.exports = api;
    else root.DeltamodUI = api;
})(typeof window === 'undefined' ? globalThis : window, function (root) {
    'use strict';
    const t = (key, fallback, ...args) => root.Localization?.t(key, fallback, ...args) ??
        String(fallback).replace(/\{(\d+)\}/g, (_, index) => String(args[index] ?? ''));
    const normalizeQuery = value => String(value ?? '').normalize('NFKD')
        .replace(/[\u0300-\u036f]/g, '').toLocaleLowerCase().trim();
    function percent(value) {
        if (!['number', 'string'].includes(typeof value) || String(value).trim() === '') return null;
        const number = Number(value);
        return Number.isFinite(number) && number >= 0 ? Math.min(100, number) : null;
    }
    function onDispose(callback) {
        (root._onClosePage ||= []).push(callback);
        return callback;
    }
    function setProgress(element, value) {
        const amount = percent(value);
        if (!element) return amount;
        if (amount === null) element.removeAttribute('value');
        else { element.max = 100; element.value = amount; }
        return amount;
    }
    function showError(target, error, retry) {
        if (!target?.isConnected) return;
        const doc = target.ownerDocument;
        const box = doc.createElement('div');
        box.className = 'workspace-load-state';
        box.setAttribute('role', 'alert');
        const heading = doc.createElement('strong');
        heading.textContent = t('ui_load_failed', 'This view could not be loaded');
        const detail = doc.createElement('p');
        detail.textContent = String(error?.message || error || t('ui_try_again', 'Please try again.'));
        box.append(heading, detail);
        if (retry) {
            const button = doc.createElement('button');
            button.type = 'button';
            button.textContent = t('ui_retry', 'Try again');
            button.onclick = () => {
                button.disabled = true;
                Promise.resolve().then(retry).catch(next => showError(target, next, retry));
            };
            box.appendChild(button);
        }
        if (target.tagName === 'TBODY') {
            const row = doc.createElement('tr');
            const cell = doc.createElement('td');
            cell.colSpan = 2;
            cell.appendChild(box);
            row.appendChild(cell);
            target.replaceChildren(row);
        } else target.replaceChildren(box);
        target.setAttribute('aria-busy', 'false');
    }
    function bindSearch({ input, rows, output, empty, predicate = () => true }) {
        if (!input) return null;
        const entries = rows.map(row => ({ row, text: normalizeQuery(row.dataset.search || row.textContent) }));
        let timer = null;
        let disposed = false;
        const apply = () => {
            if (disposed) return;
            const query = normalizeQuery(input.value);
            let count = 0;
            for (const { row, text } of entries) {
                const visible = (!query || text.includes(query)) && predicate(row);
                if (row.hidden === visible) row.hidden = !visible;
                if (visible) count += 1;
            }
            if (output) output.textContent = t('ui_result_count', '{0} of {1} shown', count, rows.length);
            if (empty) empty.hidden = count !== 0 || entries.length === 0;
        };
        const schedule = () => { clearTimeout(timer); timer = setTimeout(apply, 80); };
        const keydown = event => {
            if (event.key === 'Escape' && input.value) {
                event.preventDefault(); input.value = ''; clearTimeout(timer); apply();
            }
        };
        input.addEventListener('input', schedule);
        input.addEventListener('keydown', keydown);
        root.addEventListener?.('deltamod-language-change', apply);
        const dispose = onDispose(() => {
            disposed = true; clearTimeout(timer);
            input.removeEventListener('input', schedule);
            input.removeEventListener('keydown', keydown);
            root.removeEventListener?.('deltamod-language-change', apply);
        });
        apply();
        return { apply, dispose };
    }
    function thumbnailLoader() {
        const fallback = root.deltamodBackend.assetUrl('app', 'web/img/mod-placeholder.png');
        const pending = new Map();
        const cache = new Map();
        const queue = [];
        let active = 0;
        let disposed = false;
        const pump = () => {
            while (!disposed && active < 4 && queue.length) {
                const { image, mod } = queue.shift();
                if (!image.isConnected) continue;
                active += 1;
                let request = cache.get(mod.uid);
                if (!request) {
                    request = Promise.resolve().then(() => root.deltamodBackend.invoke('getModImage', [mod.uid]));
                    cache.set(mod.uid, request);
                }
                Promise.resolve(request).then(meta => {
                    if (!disposed && image.isConnected && meta?.path) image.src = meta.path;
                }).catch(() => {}).finally(() => { active -= 1; pump(); });
            }
        };
        const observer = typeof root.IntersectionObserver === 'function'
            ? new root.IntersectionObserver(entries => {
                for (const entry of entries) {
                    if (!entry.isIntersecting) continue;
                    const mod = pending.get(entry.target);
                    if (!mod) continue;
                    pending.delete(entry.target); observer.unobserve(entry.target);
                    queue.push({ image: entry.target, mod });
                }
                pump();
            }, { root: root.document?.querySelector('.viewport') || null, rootMargin: '240px' })
            : null;
        const add = (image, mod) => {
            image.width ||= 76; image.height ||= 76;
            image.loading = 'lazy'; image.decoding = 'async';
            image.src = mod._imagePath || fallback;
            image.onerror = () => { image.onerror = null; image.src = fallback; };
            if (mod._imagePath) return;
            if (observer) { pending.set(image, mod); observer.observe(image); }
            else { queue.push({ image, mod }); setTimeout(pump, 0); }
        };
        const dispose = onDispose(() => {
            disposed = true; observer?.disconnect(); queue.length = 0; pending.clear(); cache.clear();
        });
        return { add, dispose };
    }
    function labelIconButtons(scope) {
        for (const button of scope.querySelectorAll('button[title]')) {
            if (!button.getAttribute('aria-label')) button.setAttribute('aria-label', button.title);
        }
    }
    function mount(scope) {
        labelIconButtons(scope);
        root.DeltamodIcons?.hydrate(scope);
        for (const banner of scope.querySelectorAll('.modlist-error')) {
            banner.setAttribute('role', 'button'); banner.tabIndex = 0;
            banner.addEventListener('keydown', event => {
                if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); banner.click(); }
            });
        }
    }
    return Object.freeze({ t, percent, normalizeQuery, setProgress, showError, bindSearch, thumbnailLoader, onDispose, mount });
});
