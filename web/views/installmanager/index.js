(() => {
function setTooltip(element, { content }) {
    element.title = String(content);
    if (!element.hasAttribute('aria-label')) element.setAttribute('aria-label', String(content));
}
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
const tbody = document.querySelector('#installations-list');
const gameInfo = new Map();
(async () => {
    try {
        const installs = await window.deltamodBackend.invoke('getInstallations', []).catch(e => {
            throw new Error(`Error fetching installations: ${e.message}`);
        });
        const index = await window.deltamodBackend.invoke('getSystemIndex', []).catch(e => {
            throw new Error(`Error fetching current installation index: ${e.message}`);
        });
        if (!tbody?.isConnected) return;
        tbody.replaceChildren();
        const gameIds = [...new Set(installs.map(install => install.pid))];
        await Promise.all(gameIds.map(async pid => gameInfo.set(pid, await window.deltamodBackend.invoke('getGameInfo', [pid]))));
        if (!tbody.isConnected) return;

        for (const i in installs) {
            const install = installs[i];

            await (async (install, i) => {
                const row = document.createElement('tr');
                row.className = 'installation-row';
                const nameCell = document.createElement('td');
                const goCell = document.createElement('td');
                const buttonsDiv = document.createElement('div');
                buttonsDiv.className = 'installation-actions';

                buttonsDiv.style.display = 'flex';
                buttonsDiv.style.gap = '10px';
                buttonsDiv.style.alignItems = 'center';
                buttonsDiv.style.justifyContent = 'center';

                goCell.style.textAlign = 'center';

                console.log(JSON.stringify(install));

                const nameContainer = document.createElement('div');
                nameContainer.className = 'installation-summary';
                nameContainer.style.display = 'flex';
                nameContainer.style.justifyContent = 'left';
                nameContainer.style.alignItems = 'center';
                nameContainer.style.gap = '8px';

                const editablespan = document.createElement('input');
                editablespan.type = 'text';
                editablespan.style.display = 'block';
                editablespan.style.margin = '0';
                editablespan.style.height = '22px';
                editablespan.style.fontSize = '16px';
                editablespan.value = install.name || `Install #${install.index + 1}`;
                editablespan.setAttribute('aria-label', `Installation name: ${editablespan.value}`);
                editablespan.style.cursor = 'text';

                let savedName = editablespan.value;
                editablespan.addEventListener('keydown', event => {
                    if (event.key === 'Enter') editablespan.blur();
                    if (event.key === 'Escape') { editablespan.value = savedName; editablespan.blur(); }
                });
                editablespan.onblur = async () => {
                    if (editablespan.value.trim() === savedName) return;
                    if (editablespan.value.trim() === '') {
                        htmlAlert(
                            'Invalid installation name',
                            'This installation name is invalid. Please choose another one.',
                            [{ text: 'Ok', resolveWith: 'ok' }]
                        );

                        editablespan.value = `Install #${install.index + 1}`;
                    }

                    install.name = editablespan.value.trim();

                    try {
                        await window.deltamodBackend.invoke('setInstallationCName', [install.index.toString(), install.name]);
                        savedName = install.name;
                    } catch (error) {
                        editablespan.value = savedName;
                        if (editablespan.isConnected) await htmlAlert('Unable to rename installation', String(error?.message || error), [{ text: 'OK' }]);
                    }
                };

                const boldName = document.createElement('img');
                boldName.style.width = '43px';
                boldName.alt = ''; boldName.width = 43; boldName.height = 43; boldName.decoding = 'async';
                boldName.src = './gamesIco/' + install.pid + '.png';

                const nameText = document.createElement('div');
                nameText.className = 'installation-copy';
                nameText.appendChild(editablespan);
                nameContainer.appendChild(boldName);
                nameContainer.appendChild(nameText);

                const details = document.createElement('small');
                {
                    const gname = gameInfo.get(install.pid)?.name || 'Unknown game';

                    details.textContent = `${gname} · ${install.steam ? 'Steam' : 'Manual'}`;
                    if (!install.valid) {
                        const invalid = document.createElement('span');
                        invalid.className = 'installation-invalid';
                        invalid.textContent = `Needs attention: ${install.issues.join('; ')}`;
                        invalid.style.display = 'block';
                        invalid.style.color = '#ffb3a3';
                        invalid.style.marginTop = '4px';
                        details.appendChild(invalid);
                    }
                }

                details.classList.add('calibri');
                details.style.color = '#888';
                details.style.display = 'block';

                nameText.appendChild(details);

                console.log('created index row for install:', install.index);

                let goBtn = document.createElement('button');
                goBtn.style.padding = '4px';
                goBtn.style.textAlign = 'center';
                goBtn = adaptForIcons(goBtn);
                goBtn.innerHTML = icon('sync_arrow_up', '18px');

                goBtn.onclick = () => {
                    console.log('Switching to installation index:', install.index);

                    window.deltamodBackend.invoke('changeSystemIndex', [
                        install.index.toString(),
                    ]);
                };

                if (index == install.index || !install.valid) {
                    goBtn.disabled = true;
                    goBtn.style.cursor = 'not-allowed';
                    goBtn.style.opacity = '0.3';
                    goBtn.innerHTML = icon(index == install.index ? 'check_circle' : 'warning', '18px');
                }
                goBtn.setAttribute(
                    'aria-label',
                    !install.valid
                        ? 'Installation needs repair'
                        : index == install.index
                        ? 'Current installation'
                        : 'Switch to this installation'
                );

                buttonsDiv.appendChild(goBtn);

                setTooltip(goBtn, {
                    content:
                        !install.valid
                            ? 'Repair or re-import this installation before switching to it'
                            : index == install.index
                            ? 'Current installation'
                            : 'Switch to this installation',
                    placement: 'top',
                    delay: [500, 0],
                });

                let deleteBtn = document.createElement('button');
                deleteBtn.style.padding = '4px';
                deleteBtn.style.textAlign = 'center';
                deleteBtn = adaptForIcons(deleteBtn);
                deleteBtn.innerHTML = icon('delete', '18px');
                deleteBtn.setAttribute('aria-label', 'Delete installation');

                deleteBtn.onclick = async () => {
                    let resp = 'N';
                    if (!window.ctrlDown) {
                        resp = await htmlAlert(
                            'Warning',
                            `Are you sure you want to delete the installation "${
                                install.name || `Install #${install.index + 1}`
                            }"? This action cannot be undone.`,
                            [
                                { text: 'Yes', resolveWith: 'Y' },
                                { text: 'No', resolveWith: 'N' },
                            ]
                        );
                    }
                    else {
                        resp = 'Y';
                    }

                    if (resp === 'Y') {
                        window.deltamodBackend.invoke('deleteSystemIndex', [
                            install.index.toString(),
                        ]);
                    }
                };

                buttonsDiv.appendChild(deleteBtn);

                setTooltip(deleteBtn, {
                    content: 'Delete installation',
                    placement: 'top',
                    delay: [500, 0],
                });

                if (!install.valid) {
                    let repairBtn = document.createElement('button');
                    repairBtn.style.padding = '4px';
                    repairBtn = adaptForIcons(repairBtn);
                    repairBtn.innerHTML = icon('build', '18px');
                    repairBtn.setAttribute('aria-label', 'Attempt safe repair');
                    repairBtn.onclick = async () => {
                        const result = await window.deltamodBackend.invoke('repairInstallation', [
                            install.index.toString()
                        ]);
                        if (result.repaired) {
                            await htmlAlert('Repair complete', 'The installation is valid again.', [
                                { text: 'OK', resolveWith: 'ok' }
                            ]);
                            page('installmanager');
                        } else {
                            await htmlAlert(
                                'Repair could not finish',
                                `${result.issues.join('\n')}\n\nUse Re-import to select a clean game folder. Existing data was not deleted.`,
                                [{ text: 'OK', resolveWith: 'ok' }]
                            );
                        }
                    };
                    buttonsDiv.appendChild(repairBtn);
                    setTooltip(repairBtn, { content: 'Attempt safe repair', placement: 'top', delay: [500, 0] });

                    let reimportBtn = document.createElement('button');
                    reimportBtn.style.padding = '4px';
                    reimportBtn = adaptForIcons(reimportBtn);
                    reimportBtn.innerHTML = icon('drive_folder_upload', '18px');
                    reimportBtn.setAttribute('aria-label', 'Re-import game files');
                    reimportBtn.onclick = async () => {
                        const result = await window.deltamodBackend.invoke('reimportInstallation', [
                            install.index.toString()
                        ]);
                        if (result?.repaired) page('installmanager');
                    };
                    buttonsDiv.appendChild(reimportBtn);
                    setTooltip(reimportBtn, { content: 'Re-import from a clean game folder', placement: 'top', delay: [500, 0] });
                }

                let openBtn = document.createElement('button');
                openBtn.style.padding = '4px';
                openBtn.style.textAlign = 'center';
                openBtn = adaptForIcons(openBtn);
                openBtn.innerHTML = icon('folder_open', '18px');
                openBtn.setAttribute('aria-label', 'Open installation folder');

                openBtn.onclick = () => {
                    window.deltamodBackend.invoke('openInstallationFolder', [
                        install.index.toString(),
                    ]);
                };

                setTooltip(openBtn, {
                    content: 'Open installation folder',
                    placement: 'top',
                    delay: [500, 0],
                });

                let editBtn = document.createElement('button');
                editBtn.style.padding = '4px';
                editBtn.style.textAlign = 'center';
                editBtn = adaptForIcons(editBtn);
                editBtn.innerHTML = icon('terminal', '18px');
                editBtn.setAttribute('aria-label', 'Edit a safe game-data copy in UndertaleModTool');
                const canLaunchUndertaleModTool = window.deltamodBackend
                    .isCommandAvailable('undertaleModTool:openInstallation');
                editBtn.disabled = !install.canOpenInUndertaleModTool || !canLaunchUndertaleModTool;
                if (!canLaunchUndertaleModTool) {
                    editBtn.title = 'UndertaleModTool launch is unavailable in this app build';
                }
                editBtn.onclick = async () => {
                    try {
                        const result = await window.communityAPI.tools
                            .openInstallationInUndertaleModTool(install.index.toString());
                        if (!result?.launched && !result?.canceled) {
                            throw new Error('UndertaleModTool did not start.');
                        }
                    } catch (error) {
                        await htmlAlert(
                            'Could not open UndertaleModTool',
                            error?.message || String(error),
                            [{ text: 'OK', resolveWith: 'ok' }]
                        );
                    }
                };
                setTooltip(editBtn, {
                    content: install.canOpenInUndertaleModTool
                        ? (canLaunchUndertaleModTool
                            ? 'Create a safe copy for UndertaleModTool; export changes back as a Community mod'
                            : 'UndertaleModTool launch is unavailable in this app build')
                        : 'Repair this installation before creating an UndertaleModTool workspace',
                    placement: 'top',
                    delay: [500, 0],
                });

                let shortcutBtn = document.createElement('button');
                shortcutBtn.style.padding = '4px';
                shortcutBtn.style.textAlign = 'center';
                shortcutBtn = adaptForIcons(shortcutBtn);
                shortcutBtn.innerHTML = icon('forward', '18px');
                shortcutBtn.title = 'Create shortcut on desktop';
                shortcutBtn.setAttribute('aria-label', 'Create shortcut on desktop');
                const canCreateShortcut = window.deltamodBackend.isCommandAvailable('createInstallLink');
                shortcutBtn.disabled = !canCreateShortcut;
                if (!canCreateShortcut) {
                    shortcutBtn.title = 'Desktop shortcut creation is unavailable in this app build';
                }

                shortcutBtn.onclick = async () => {
                    if (!(await window.deltamodBackend.invoke('isPackaged', []))) {
                        await htmlAlert(
                            'Error',
                            'This feature is only available when Deltamod is packaged.',
                            [{ text: 'Ok', resolveWith: 'ok' }]
                        );
                        return;
                    }

                    await window.deltamodBackend.invoke('createInstallLink', [
                        install.index.toString(),
                        install.name || `Install #${install.index + 1}`,
                    ]);
                };

                setTooltip(shortcutBtn, {
                    content: canCreateShortcut
                        ? 'Create shortcut on desktop'
                        : 'Desktop shortcut creation is unavailable in this app build',
                    placement: 'top',
                    delay: [500, 0],
                });

                buttonsDiv.appendChild(shortcutBtn);
                buttonsDiv.appendChild(openBtn);
                buttonsDiv.appendChild(editBtn);

                goCell.appendChild(buttonsDiv);

                nameCell.appendChild(nameContainer);

                row.appendChild(nameCell);
                row.appendChild(goCell);

                if (tbody.isConnected) tbody.appendChild(row);
            })(install, i).catch(e => {
                throw new Error(`Error creating row for installation ${install.index}: ${e.message}`);
            });
        }

        const newRow = document.createElement('tr');
        newRow.className = 'installation-create-row';
        const newCell = document.createElement('td');

        newCell.colSpan = 2;
        newCell.style.textAlign = 'center';

        const newButton = document.createElement('button');
        newButton.style.width = '100%';
        newButton.style.cursor = 'pointer';
        newButton.style.display = 'inline-flex';
        newButton.style.alignItems = 'center';
        newButton.style.gap = '10px';
        newButton.style.justifyContent = 'center';
        newButton.style.textAlign = 'center';

        newButton.innerHTML =
            icon('create_new_folder', '18px') + ' Create new installation';

        newButton.onclick = () => {
            window.fromIM = true;
            page('locate');
        };

        newCell.appendChild(newButton);
        newRow.appendChild(newCell);

        if (!tbody.isConnected) return;
        tbody.appendChild(newRow);
        tbody.setAttribute('aria-busy', 'false');

        genbtnstyles();
    } catch (error) {
        window.DeltamodUI.showError(tbody, error, () => page('installmanager'));
    }
})();

elisten(document, 'keydown', e => {
    var ctrlDown = e.ctrlKey || e.metaKey;
    window.ctrlDown = ctrlDown;
});

elisten(document, 'keyup', e => {
    var ctrlDown = e.ctrlKey || e.metaKey;
    window.ctrlDown = ctrlDown;
});
})();
