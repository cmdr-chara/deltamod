/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    'use strict';
    const table = document.getElementById('collections');
    const form = document.getElementById('collection-create-form');
    const input = document.getElementById('collection-name');
    const create = document.getElementById('collection-create');
    const status = document.getElementById('collection-status');
    const ui = window.DeltamodUI;
    let busy = false;
    const errorText = error => String(error?.message || error?.error || error || 'Please try again.');
    const check = response => {
        if (response?.success === false) throw new Error(errorText(response));
        return response;
    };
    function action(label, glyph, callback, available = true) {
        const button = document.createElement('button');
        button.type = 'button'; button.className = 'secondary-action';
        button.innerHTML = icon(glyph, '1.1em');
        button.title = label; button.setAttribute('aria-label', label);
        button.disabled = !available;
        button.onclick = async () => {
            if (button.disabled) return;
            button.disabled = true;
            try { await callback(); }
            catch (error) { await htmlAlert('Collection unavailable', errorText(error), [{ text: 'OK' }]); }
            finally { if (button.isConnected) button.disabled = !available; }
        };
        return button;
    }
    async function load() {
        table.setAttribute('aria-busy', 'true');
        try {
            const collections = check(await invoke('gamebanana_getCollections', []));
            if (!table.isConnected) return;
            if (!Array.isArray(collections)) throw new Error('The collection service returned an invalid response.');
            const fragment = document.createDocumentFragment();
            for (const collection of collections) {
                const row = document.createElement('tr');
                const name = document.createElement('td'); name.textContent = String(collection.name);
                const actions = document.createElement('td'); actions.className = 'actions';
                actions.append(action('Open collection on GameBanana', 'open_in_new', () => {
                    window.open(`https://gamebanana.com/collections/${encodeURIComponent(collection.id)}`, '_blank');
                }));
                actions.append(action('Add installed mods to collection', 'bottom_panel_open', async () => {
                    window._pageArguments = { collectionId: collection.id };
                    await page('collection-exportchoose');
                }));
                const canRestore = window.deltamodBackend.isCommandAvailable('gamebanana_downloadAllInCollection');
                actions.append(action(canRestore ? 'Restore collection' : 'Collection restore is unavailable in this app build', 'bottom_panel_close', async () => {
                    const response = check(await invoke('gamebanana_downloadAllInCollection', [collection.id]));
                    await htmlAlert('Collection restored', `Skipped ${response?.skipped ?? response?.skippedMods ?? 0} mods.`, [{ text: 'OK' }]);
                }, canRestore));
                const remove = action('Delete collection', 'delete', async () => {
                    const choice = await htmlAlert('Delete collection', `Delete “${collection.name}”? This cannot be undone. Installed mod files will not be removed.`, [
                        { text: 'Delete collection', resolveWith: 'delete' }, { text: 'Cancel', resolveWith: false }
                    ]);
                    if (choice !== 'delete' || !table.isConnected) return;
                    check(await invoke('gamebanana_deleteCollection', [collection.id]));
                    await load();
                });
                remove.classList.add('quiet-danger'); actions.append(remove);
                row.append(name, actions); fragment.append(row);
            }
            if (!collections.length) {
                const row = document.createElement('tr'); const cell = document.createElement('td');
                cell.colSpan = 2; cell.className = 'workspace-load-state';
                cell.textContent = ui.t('ui_collection_empty', 'No collections yet. Create one above to save a mod setup.');
                row.append(cell); fragment.append(row);
            }
            table.replaceChildren(fragment); ui.mount(table);
        } catch (error) { ui.showError(table, error, load); }
        finally { table.setAttribute('aria-busy', 'false'); }
    }
    form.addEventListener('submit', async event => {
        event.preventDefault();
        if (busy || !form.reportValidity()) return;
        const name = input.value.trim();
        if (!name) { input.focus(); return; }
        busy = true; create.disabled = true; form.setAttribute('aria-busy', 'true'); status.textContent = '';
        try {
            check(await invoke('gamebanana_createCollection', [name]));
            if (!form.isConnected) return;
            input.value = ''; await load(); input.focus();
        } catch (error) { status.textContent = errorText(error); }
        finally { busy = false; create.disabled = false; form.removeAttribute('aria-busy'); }
    });
    void load();
})();
