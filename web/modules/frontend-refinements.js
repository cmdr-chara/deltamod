// Copyright © 2026 cmdr-chara. Licensed under the EUPL 1.2.
// Small controllers for the original table-based UI; no replacement shell.
(function (root, factory) {
    const api = factory(root);
    if (typeof module !== 'undefined' && module.exports) module.exports = api;
    if (root) root.FrontendRefinements = api;
})(typeof window !== 'undefined' ? window : globalThis, root => {
    'use strict';
    const text = value => String(value ?? '').normalize('NFKC').toLocaleLowerCase();
    const t = (key, fallback, ...args) => root.Localization?.t(key, fallback, ...args)
        ?? args.reduce((value, arg, i) => value.replaceAll(`{${i}}`, String(arg)), fallback);
    const onClose = cleanup => (root._onClosePage ||= []).push(cleanup);

    function matches(mod, query) {
        const haystack = text([mod.name, mod.author, mod.packageID, mod.version].flat().join(' '));
        return text(query).trim().split(/\s+/).every(word => haystack.includes(word));
    }

    function compareMods(a, b, order, collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })) {
        const name = () => collator.compare(String(a.name ?? ''), String(b.name ?? ''));
        if (order === 'desc') return -name();
        if (order === 'size-asc') return (Number(a.size) || 0) - (Number(b.size) || 0) || name();
        if (order === 'size-desc') return (Number(b.size) || 0) - (Number(a.size) || 0) || name();
        if (order === 'author') {
            const author = mod => String(Array.isArray(mod.author) ? mod.author[0] ?? '' : mod.author ?? '');
            return collator.compare(author(a), author(b)) || name();
        }
        return name();
    }

    // Reorder the existing rows, never reconstruct controls or reload the native list.
    // Hidden rows stay in the DOM so enabled mods/variants remain part of patch planning.
    function tableTools(tbody, { input, clear, count, sort, filter } = {}) {
        const records = [];
        const listeners = [];
        let groups = [];
        let done = false;
        let disposed = false;
        let predicate = () => true;
        const listen = (node, event, handler) => {
            if (!node) return;
            node.addEventListener(event, handler);
            listeners.push(() => node.removeEventListener(event, handler));
        };
        const emptyRow = root.document.createElement('tr');
        const emptyCell = root.document.createElement('td');
        emptyCell.colSpan = 2;
        emptyCell.className = 'mod-search-empty calibri';
        emptyRow.append(emptyCell);
        emptyRow.hidden = true;
        const refresh = () => {
            if (disposed || !tbody.isConnected) return;
            const query = input?.value || '';
            let visible = 0;
            records.forEach(({ mod, row }) => {
                row.hidden = !(predicate(mod) && matches(mod, query));
                if (!row.hidden) visible++;
            });
            groups.forEach(({ row, members }) => { row.hidden = members.every(record => record.row.hidden); });
            if (clear) clear.hidden = query.length === 0;
            if (count) count.textContent = t('refine_mod_count', '{0} of {1} mods', visible, records.length);
            emptyCell.textContent = t('refine_no_matches', 'No matching mods. Clear the search or change the filter.');
            emptyRow.hidden = !done || visible !== 0 || records.length === 0;
            if (done && !emptyRow.isConnected) tbody.append(emptyRow);
        };
        const reorder = () => {
            const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
            records.sort((a, b) => compareMods(a.mod, b.mod, sort?.value, collator));
            groups.forEach(group => group.row.remove());
            groups = [];
            const fragment = root.document.createDocumentFragment();
            let group;
            records.forEach(record => {
                if (sort?.value === 'author') {
                    const author = String(Array.isArray(record.mod.author) ? record.mod.author[0] || 'Unknown' : record.mod.author || 'Unknown');
                    if (!group || group.author !== author) {
                        const row = root.document.createElement('tr');
                        const cell = root.document.createElement('th');
                        cell.colSpan = 2;
                        cell.textContent = author;
                        row.className = 'mod-author-heading';
                        row.append(cell);
                        group = { author, row, members: [] };
                        groups.push(group);
                        fragment.append(row);
                    }
                    group.members.push(record);
                }
                fragment.append(record.row);
            });
            tbody.append(fragment, emptyRow);
            refresh();
        };
        listen(input, 'input', refresh);
        listen(input, 'keydown', event => {
            if (event.key === 'Escape') { event.preventDefault(); input.value = ''; refresh(); }
        });
        listen(clear, 'click', () => { input.value = ''; refresh(); input.focus(); });
        listen(sort, 'change', reorder);
        listen(filter, 'change', () => {
            predicate = mod => filter.value === 'all' || mod.game === filter.value;
            refresh();
        });
        if (filter) predicate = mod => filter.value === 'all' || mod.game === filter.value;
        listen(root, 'deltamod-language-change', refresh);
        onClose(() => { disposed = true; listeners.forEach(remove => remove()); records.length = 0; });
        return {
            add(mod, row) { if (!disposed && row) records.push({ mod, row }); },
            finish() { done = true; if (sort) reorder(); else refresh(); },
            refresh
        };
    }

    // Only resolve artwork as it approaches the scroll viewport. At most four native
    // image lookups run at once; navigation drops queued work and late results.
    function artworkLoader() {
        let disposed = false;
        let active = 0;
        const queue = [];
        const jobs = new WeakMap();
        const pump = () => {
            if (disposed) return;
            while (active < 4 && queue.length) {
                const { img, uid } = queue.shift();
                if (!img.isConnected) continue;
                active++;
                Promise.resolve().then(() => root.deltamodBackend.invoke('getModImage', [uid]))
                    .then(meta => { if (!disposed && img.isConnected && meta?.path) img.src = meta.path; })
                    .catch(() => {}) // Keep the already rendered local placeholder.
                    .finally(() => { active--; pump(); });
            }
        };
        const observer = typeof root.IntersectionObserver === 'function'
            ? new root.IntersectionObserver(entries => {
                entries.forEach(entry => {
                    if (!entry.isIntersecting) return;
                    observer.unobserve(entry.target);
                    const job = jobs.get(entry.target);
                    if (job) { jobs.delete(entry.target); queue.push(job); }
                });
                pump();
            }, { rootMargin: '240px' }) : null;
        onClose(() => { disposed = true; queue.length = 0; observer?.disconnect(); });
        return (img, uid) => {
            img.loading = 'lazy';
            img.decoding = 'async';
            const job = { img, uid };
            if (observer) { jobs.set(img, job); observer.observe(img); }
            else { queue.push(job); pump(); }
        };
    }

    // Keep feedback local to the control, without replacing the settings layout.
    async function saveControl(control, save, rollback) {
        if (control.disabled) return false;
        let status = control.parentElement.querySelector('.control-save-status');
        if (!status) {
            status = root.document.createElement('small');
            status.className = 'control-save-status calibri';
            status.setAttribute('role', 'status');
            control.parentElement.append(status);
        }
        control.disabled = true;
        control.setAttribute('aria-busy', 'true');
        status.textContent = t('refine_saving', 'Saving…');
        status.classList.remove('is-error');
        try {
            await save();
            status.textContent = t('refine_saved', 'Saved');
            control.removeAttribute('aria-invalid');
            return true;
        } catch (error) {
            rollback?.();
            control.setAttribute('aria-invalid', 'true');
            status.classList.add('is-error');
            status.textContent = t('refine_save_failed', 'Not saved. Try again.');
            status.title = String(error?.message || error);
            return false;
        } finally {
            control.disabled = false;
            control.removeAttribute('aria-busy');
        }
    }

    function showListError(tbody, error) {
        if (!tbody?.isConnected) return;
        const row = root.document.createElement('tr');
        const cell = root.document.createElement('td');
        cell.colSpan = 2;
        cell.className = 'mod-search-empty calibri';
        const message = root.document.createElement('p');
        message.setAttribute('role', 'alert');
        message.textContent = t('refine_load_failed', 'Could not load the mod list.');
        const detail = root.document.createElement('small');
        detail.textContent = String(error?.message || error);
        const retry = root.document.createElement('button');
        retry.type = 'button';
        retry.textContent = t('refine_retry', 'Retry');
        retry.addEventListener('click', () => root.page(''));
        cell.append(message, detail, root.document.createElement('br'), retry);
        row.append(cell);
        tbody.replaceChildren(row);
        tbody.closest('table')?.setAttribute('aria-busy', 'false');
        const toolbar = tbody.closest('.mods-page')?.querySelector('.mod-search-toolbar');
        if (toolbar) toolbar.hidden = true;
    }

    return { matches, compareMods, tableTools, artworkLoader, saveControl, showListError };
});
