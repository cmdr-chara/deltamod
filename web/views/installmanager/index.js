(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
(async () => {
    try {
        const installs = await window.deltamodBackend.invoke('getInstallations', []).catch(e => {
            throw new Error(`Error fetching installations: ${e.message}`);
        });
        const index = await window.deltamodBackend.invoke('getSystemIndex', []).catch(e => {
            throw new Error(`Error fetching current installation index: ${e.message}`);
        });
        const tbody = document.querySelector('#installations-list');

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
                editablespan.value = sanitizeHTML(
                    install.name || `Install #${install.index + 1}`
                );
                editablespan.style.cursor = 'text';

                editablespan.onblur = async () => {
                    if (editablespan.value.trim() === '') {
                        htmlAlert(
                            'Invalid installation name',
                            'This installation name is invalid. Please choose another one.',
                            [{ text: 'Ok', resolveWith: 'ok' }]
                        );

                        editablespan.value = `Install #${install.index + 1}`;
                    }

                    install.name = editablespan.value.trim();

                    window.deltamodBackend.invoke('setInstallationCName', [
                        install.index.toString(),
                        install.name,
                    ]);
                };

                const boldName = document.createElement('img');
                boldName.style.width = '43px';
                boldName.src = './gamesIco/' + install.pid + '.png';

                const nameText = document.createElement('div');
                nameText.className = 'installation-copy';
                nameText.appendChild(editablespan);
                nameContainer.appendChild(boldName);
                nameContainer.appendChild(nameText);

                const details = document.createElement('small');
                {
                    const gname = await window.electronAPI
                        .invoke('getGameInfo', [install.pid])
                        .then(g => g?.name || 'Unknown game');

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

                tippy(goBtn, {
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

                tippy(deleteBtn, {
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
                    tippy(repairBtn, { content: 'Attempt safe repair', placement: 'top', delay: [500, 0] });

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
                    tippy(reimportBtn, { content: 'Re-import from a clean game folder', placement: 'top', delay: [500, 0] });
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

                tippy(openBtn, {
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
                tippy(editBtn, {
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

                tippy(shortcutBtn, {
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

                tbody.appendChild(row);
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

        tbody.appendChild(newRow);

        genbtnstyles();
    } catch (e) {
        var errorTR = document.createElement('tr');
        var errorTD = document.createElement('td');
        errorTD.colSpan = 2;
        errorTD.style.fontWeight = 'bold';

        var errortitle = document.createElement('div');
        errortitle.innerText = 'Error loading installations';
        errortitle.style.fontSize = '18px';
        errortitle.style.marginBottom = '10px';
        errorTD.appendChild(errortitle);

        var errorMsg = document.createElement('div');
        errorMsg.textContent = [e?.message, e?.stack].filter(Boolean).join('\n') || String(e);
        errorMsg.style.whiteSpace = 'pre-wrap';
        errorMsg.style.fontSize = '14px';
        errorTD.appendChild(errorMsg);

        errorTR.appendChild(errorTD);
        document.querySelector('#installations-list').appendChild(errorTR);
    }
})().catch(e => {
    window.alert('Unexpected error: ' + e.message + '\n' + e.stack);
});

elisten(document, 'keydown', e => {
    var ctrlDown = e.ctrlKey || e.metaKey;
    window.ctrlDown = ctrlDown;
});

elisten(document, 'keyup', e => {
    var ctrlDown = e.ctrlKey || e.metaKey;
    window.ctrlDown = ctrlDown;
});
})();
