/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    'use strict';
    const table = document.getElementById('mod-list');
    const button = document.getElementById('export-btn');
    const collectionId = window._pageArguments?.collectionId;
    const thumbnails = window.DeltamodUI.thumbnailLoader();
    let exporting = false;
    async function load() {
        const { modList } = await invoke('getModList', []);
        if (!table.isConnected) return;
        if (!Array.isArray(modList)) throw new Error('The mod catalogue could not be read.');
        const fragment = document.createDocumentFragment();
        for (const mod of modList) {
            const row = document.createElement('tr'); const cell = document.createElement('td');
            const identity = document.createElement('label'); identity.className = 'export-mod-identity';
            const image = document.createElement('img'); image.alt = ''; image.width = 40; image.height = 40;
            thumbnails.add(image, mod);
            const text = document.createElement('span'); text.textContent = String(mod.name);
            identity.append(image, text); cell.append(identity);
            const selection = document.createElement('td'); selection.className = 'export-selection';
            const checkbox = document.createElement('input'); checkbox.type = 'checkbox';
            checkbox.id = `export-mod-${fragment.childElementCount}`; identity.htmlFor = checkbox.id;
            checkbox.setAttribute('aria-label', `Export ${mod.name}`);
            const supported = mod.gamebanana?.supports === true;
            checkbox.disabled = !supported;
            if (supported) {
                checkbox.dataset.model = mod.gamebanana.model; checkbox.dataset.id = mod.gamebanana.id;
                checkbox.dataset.name = mod.name; checkbox.dataset.pid = mod.packageID;
            } else {
                const note = document.createElement('small'); note.className = 'task-note';
                note.textContent = 'Only mods downloaded from GameBanana are supported'; text.append(document.createElement('br'), note);
            }
            selection.append(checkbox); row.append(cell, selection); fragment.append(row);
        }
        if (!modList.length) {
            const row = document.createElement('tr'); const cell = document.createElement('td'); cell.colSpan = 2;
            cell.textContent = 'No installed mods to export.'; cell.className = 'workspace-load-state'; row.append(cell); fragment.append(row);
        }
        table.replaceChildren(fragment); table.setAttribute('aria-busy', 'false');
    }
    table.addEventListener('change', () => {
        button.disabled = exporting || !table.querySelector('input:checked:not(:disabled)') || collectionId == null;
    });
    window.currentPageStack.exportMods = async () => {
        if (exporting || button.disabled) return;
        const selectedMods = [...table.querySelectorAll('input:checked:not(:disabled)')].map(checkbox => ({
            model: checkbox.dataset.model, id: checkbox.dataset.id, name: checkbox.dataset.name, pid: checkbox.dataset.pid
        }));
        exporting = true; button.disabled = true; button.setAttribute('aria-busy', 'true');
        try {
            const response = await invoke('gamebanana_importToCollection', [collectionId, selectedMods]);
            if (response?.success === false) throw new Error(String(response.message || response.error || 'Export failed.'));
            if (!table.isConnected) return;
            await htmlAlert('Collection updated', 'The selected mods have been added to the collection.', [{ text: 'OK' }]);
            window._pageArguments = {}; await page('collections');
        } catch (error) { await htmlAlert('Unable to export', String(error?.message || error), [{ text: 'OK' }]); }
        finally { exporting = false; button.removeAttribute('aria-busy'); button.disabled = !table.querySelector('input:checked:not(:disabled)'); }
    };
    if (collectionId == null) window.DeltamodUI.showError(table, 'Choose a collection before exporting.', () => page('collections'));
    else load().catch(error => window.DeltamodUI.showError(table, error, load));
})();
