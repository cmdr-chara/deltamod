// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

(() => {
    'use strict';

    const PRODUCT_UI_SCRIPT = './modules/product-ui.js';
    const PRODUCT_UI_STYLES = './modules/product-ui.css';

    // The accepted contracts-v1 golden documents remain available only for an
    // explicit preview/test flag. Production reads the existing local catalogue.
    const contractsV1Fixture = {
        installedMod: {
            archiveSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            displayName: 'Fixture Mod',
            documentKind: 'installed_mod',
            filePlanFingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            files: [
                {
                    expectedSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    path: 'mods/fixture.dat',
                    pathIdentityKey: 'mods/fixture.dat'
                }
            ],
            installationId: 'fixture-installation',
            installedAtMs: 1700000000000,
            instanceId: 'fixture-instance',
            manifestGeneration: 1,
            modId: 'fixture-mod',
            provider: {
                artifactId: '5678',
                artifactKind: 'file',
                canonicalUrl: 'https://gamebanana.com/mods/1234',
                itemKind: 'mod',
                providerId: 'gamebanana',
                resourceId: '1234',
                scope: null,
                versionId: '1.0.0'
            },
            schemaVersion: 1,
            updatedAtMs: 1700000000000,
            version: '1.0.0'
        },
        providerDescriptor: {
            authentication: 'optional',
            capabilities: ['search', 'details', 'direct_download'],
            displayName: 'GameBanana',
            documentKind: 'provider_descriptor',
            providerId: 'gamebanana',
            schemaVersion: 1
        },
        verificationResult: {
            checkedFiles: 1,
            documentKind: 'verification_result',
            installationId: 'fixture-installation',
            issues: [],
            modInstanceId: 'fixture-instance',
            schemaVersion: 1,
            state: 'healthy',
            verifiedAtMs: 1700000000200
        },
        gameHealthReport: {
            checkedAtMs: 1700000000300,
            documentKind: 'game_health_report',
            installationId: 'fixture-installation',
            interruptedOperations: [],
            lifecycleOwnedFiles: 1,
            schemaVersion: 1,
            state: 'healthy',
            unknownModifiedFiles: 0
        },
        conflictReport: {
            conflicts: [
                {
                    actualSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    existingOwners: ['fixture-instance'],
                    expectedSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    path: 'mods/fixture.dat',
                    pathIdentityKey: 'mods/fixture.dat',
                    proposedSha256: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                    reason: 'different_content',
                    requestingOwner: 'other-instance'
                }
            ],
            documentKind: 'conflict_report',
            installationId: 'fixture-installation',
            schemaVersion: 1
        },
        operationProgress: {
            cancellable: false,
            completed: 1,
            currentItem: null,
            documentKind: 'operation_progress',
            installationId: 'fixture-installation',
            kind: 'install',
            operationId: 'operation-1',
            phase: 'complete',
            schemaVersion: 1,
            state: 'succeeded',
            total: 1,
            updatedAtMs: 1700000000400
        },
        operationRecord: {
            createdAtMs: 1700000000000,
            documentKind: 'operation_record',
            error: null,
            fencingToken: 1,
            phase: 'complete',
            request: {
                idempotencyKey: 'request-1',
                intent: {
                    archiveSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    filePlanFingerprint: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    installationId: 'fixture-installation',
                    kind: 'install',
                    modInstanceId: 'fixture-instance',
                    profileId: null,
                    provider: {
                        artifactId: '5678',
                        artifactKind: 'file',
                        canonicalUrl: 'https://gamebanana.com/mods/1234',
                        itemKind: 'mod',
                        providerId: 'gamebanana',
                        resourceId: '1234',
                        scope: null,
                        versionId: '1.0.0'
                    }
                },
                operationId: 'operation-1',
                requestFingerprint: 'af1ba1dbb5846018f33f353445d9b9f9a74ddae373c6c67cc52268b67bee9702'
            },
            resultFingerprint: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
            revision: 2,
            schemaVersion: 1,
            state: 'succeeded',
            updatedAtMs: 1700000000400
        },
        lifecycleJournal: {
            backupRoot: {
                canonicalPathSha256: 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
                fileId: 'file-backup',
                volumeId: 'volume-backup'
            },
            documentKind: 'lifecycle_journal',
            fencingToken: 1,
            idempotencyKey: 'request-1',
            installationId: 'fixture-installation',
            journalSequence: 7,
            leaseId: 'lease-1',
            manifestCommitState: 'published',
            manifestGenerationAfter: 2,
            manifestGenerationBefore: 1,
            mutations: [{
                action: 'replace',
                backupPath: 'backups/fixture.dat',
                backupSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                checkpoint: 'output_verified',
                expectedSha256: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                index: 0,
                path: 'mods/fixture.dat',
                pathIdentityKey: 'mods/fixture.dat',
                previousSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                stagingPath: 'staged/fixture.dat',
                stagingSha256: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
            }],
            operation: 'install',
            operationId: 'operation-1',
            operationRevision: 2,
            phase: 'complete',
            pinned: false,
            recoveryAttempts: 0,
            recoveryChainSha256: 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
            recoveryGenerationId: 'recovery-1',
            requestFingerprint: 'af1ba1dbb5846018f33f353445d9b9f9a74ddae373c6c67cc52268b67bee9702',
            schemaVersion: 1,
            stagingRoot: {
                canonicalPathSha256: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                fileId: 'file-staging',
                volumeId: 'volume-staging'
            },
            startedAtMs: 1700000000000,
            transactionRoot: {
                canonicalPathSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                fileId: 'file-transaction',
                volumeId: 'volume-transaction'
            },
            updatedAtMs: 1700000000100
        },
        productError: {
            code: 'installation_busy',
            documentKind: 'product_error',
            messageKey: 'operation.installation_busy',
            operationId: 'operation-1',
            phase: 'preflight',
            recoveryAction: 'no_action',
            retryable: false,
            safeDetails: { activeOperation: 'operation-0' },
            schemaVersion: 1
        }
    };

    const fixtureUiOverlay = {
        artworkByInstanceId: {
            'fixture-instance': './img/mod-placeholder.png'
        },
        // Update discovery is not part of InstalledModRecord v1. This fixture-only
        // overlay exercises the product state without extending the wire contract.
        availableVersionByInstanceId: {
            'fixture-instance': '1.1.0'
        },
        runtime: {
            version: '2.0.13',
            shell: window.__TAURI_INTERNALS__ ? 'tauri' : 'electron',
            platform: /Windows/i.test(window.navigator?.userAgent || '')
                ? 'windows'
                : /Macintosh|Mac OS X/i.test(window.navigator?.userAgent || '')
                    ? 'macos'
                    : /Linux/i.test(window.navigator?.userAgent || '')
                        ? 'linux'
                        : 'unknown'
        }
    };

    function ensureStylesheet() {
        const existing = document.querySelector('link[data-product-ui-styles]');
        if (existing) return Promise.resolve(existing);

        return new Promise((resolve, reject) => {
            const link = document.createElement('link');
            link.rel = 'stylesheet';
            link.href = PRODUCT_UI_STYLES;
            link.setAttribute('data-product-ui-styles', '');
            link.addEventListener('load', () => resolve(link), { once: true });
            link.addEventListener(
                'error',
                () => reject(new Error('Unable to load Installed Mods v2 styles.')),
                { once: true }
            );
            document.head.appendChild(link);
        });
    }

    function ensureProductUi() {
        if (window.DeltamodProductUI) return Promise.resolve(window.DeltamodProductUI);

        const existing = document.querySelector('script[data-product-ui-module]');
        if (existing) {
            return new Promise((resolve, reject) => {
                existing.addEventListener(
                    'load',
                    () => resolve(window.DeltamodProductUI),
                    { once: true }
                );
                existing.addEventListener(
                    'error',
                    () => reject(new Error('Unable to load Installed Mods v2 components.')),
                    { once: true }
                );
            });
        }

        return new Promise((resolve, reject) => {
            const script = document.createElement('script');
            script.src = PRODUCT_UI_SCRIPT;
            script.async = false;
            script.setAttribute('data-product-ui-module', '');
            script.addEventListener('load', () => {
                if (window.DeltamodProductUI) resolve(window.DeltamodProductUI);
                else reject(new Error('Installed Mods v2 components did not initialize.'));
            }, { once: true });
            script.addEventListener(
                'error',
                () => reject(new Error('Unable to load Installed Mods v2 components.')),
                { once: true }
            );
            document.body.appendChild(script);
        });
    }

    function renderFallbackError(root, error) {
        const panel = document.createElement('section');
        panel.className = 'product-state product-state-error';
        panel.setAttribute('role', 'alert');
        panel.setAttribute('data-state', 'error');
        const heading = document.createElement('h2');
        heading.textContent = 'Unable to load installed mods';
        const message = document.createElement('p');
        message.textContent = error instanceof Error
            ? error.message
            : 'The installed mod catalogue could not be displayed.';
        panel.append(heading, message);
        root.replaceChildren(panel);
        root.setAttribute('aria-busy', 'false');
    }

    async function copyDiagnostics(text) {
        const payload = String(text);
        if (window.navigator?.clipboard?.writeText) {
            await window.navigator.clipboard.writeText(payload);
            return;
        }

        const copyTarget = document.createElement('textarea');
        copyTarget.value = payload;
        copyTarget.readOnly = true;
        copyTarget.setAttribute('aria-hidden', 'true');
        Object.assign(copyTarget.style, {
            position: 'fixed',
            inset: '-10000px auto auto -10000px'
        });
        document.body.appendChild(copyTarget);
        copyTarget.select();
        const copied = document.execCommand?.('copy') === true;
        copyTarget.remove();
        if (!copied) throw new Error('Clipboard access is unavailable.');
    }

    async function start() {
        const root = document.getElementById('installed-mods-v2-root');
        if (!root) return;

        try {
            const [, ProductUI] = await Promise.all([ensureStylesheet(), ensureProductUi()]);
            if (!root.isConnected) return;

            const fixturePreview = window.__DELTAMOD_PRODUCT_UI_FIXTURES__ === true;
            const invoke = (channel, args) => {
                if (typeof window.deltamodBackend?.invoke !== 'function') {
                    throw new Error('Installed mod data is unavailable in this app build.');
                }
                return window.deltamodBackend.invoke(channel, args);
            };
            const notifications = ProductUI.NotificationCenter({
                document,
                label: 'Installed mod operations'
            });
            root.parentNode?.appendChild(notifications.element);
            const profileList = document.getElementById('profile-list');
            const profileCreate = document.getElementById('profile-create');
            const profileImport = document.getElementById('profile-import');
            const profileImportFile = document.getElementById('profile-import-file');
            const commandAvailable = channel =>
                window.deltamodBackend?.isCommandAvailable?.(channel) !== false;
            const profileId = profile => String(profile?.profileId || profile?.profile_id || '');
            const renderProfiles = async () => {
                if (!profileList) return;
                if (!commandAvailable('lifecycle:listProfiles')) {
                    profileList.replaceChildren();
                    const unavailable = document.createElement('p');
                    unavailable.className = 'product-profile-empty';
                    unavailable.textContent = 'Profiles require the transactional Rust runtime.';
                    profileList.appendChild(unavailable);
                    return;
                }
                try {
                    const response = await invoke('lifecycle:listProfiles', []);
                    const profiles = Array.isArray(response?.profiles) ? response.profiles : [];
                    const activeByInstallation = new Map();
                    if (commandAvailable('lifecycle:getActiveProfile')) {
                        const installationIds = [...new Set(profiles
                            .map(profile => profile?.installationId || profile?.installation_id)
                            .filter(Boolean))];
                        await Promise.all(installationIds.map(async installationId => {
                            const active = await invoke('lifecycle:getActiveProfile', [installationId]);
                            activeByInstallation.set(installationId, active?.activeProfile || null);
                        }));
                    }
                    profileList.replaceChildren();
                    if (profiles.length === 0) {
                        const empty = document.createElement('p');
                        empty.className = 'product-profile-empty';
                        empty.textContent = 'No saved profiles yet.';
                        profileList.appendChild(empty);
                        return;
                    }
                    for (const profile of profiles) {
                        const id = profileId(profile);
                        const installationId = profile?.installationId || profile?.installation_id;
                        const active = activeByInstallation.get(installationId);
                        const isActive = active?.profileId === id || active?.profile_id === id;
                        const row = document.createElement('article');
                        row.className = `product-profile-row${isActive ? ' is-active' : ''}`;
                        const copy = document.createElement('div');
                        const heading = document.createElement('h3');
                        heading.textContent = id || 'Unnamed profile';
                        if (isActive) {
                            const badge = document.createElement('span');
                            badge.className = 'product-profile-active';
                            badge.textContent = 'Active';
                            heading.append(' ', badge);
                        }
                        const summary = document.createElement('p');
                        const mods = profile?.mods?.length ?? profile?.lockedMods?.length ?? 0;
                        summary.textContent = `${mods} exact mod${mods === 1 ? '' : 's'} · ${profile?.gameId || profile?.game_id || 'game'}`;
                        copy.append(heading, summary);
                        const actions = document.createElement('div');
                        actions.className = 'product-profile-row-actions';
                        const exportButton = document.createElement('button');
                        exportButton.type = 'button';
                        exportButton.textContent = 'Export';
                        exportButton.addEventListener('click', async () => {
                            try {
                                const exported = await invoke('lifecycle:exportProfileLockfile', [id]);
                                const blob = new Blob([exported.canonicalJson], { type: 'application/json' });
                                const url = URL.createObjectURL(blob);
                                const link = document.createElement('a');
                                link.href = url;
                                link.download = exported.fileName;
                                link.click();
                                URL.revokeObjectURL(url);
                            } catch (error) {
                                notifications.notify({
                                    title: 'Export profile',
                                    message: 'The canonical lockfile could not be exported.',
                                    tone: 'error'
                                });
                            }
                        });
                        const activateButton = document.createElement('button');
                        activateButton.type = 'button';
                        activateButton.textContent = 'Activate';
                        activateButton.disabled = isActive || !commandAvailable('lifecycle:switchProfile');
                        if (isActive) activateButton.textContent = 'Active';
                        if (activateButton.disabled) {
                            activateButton.title = isActive
                                ? 'This profile already matches the active transactional manifest.'
                                : 'Profile switching is unavailable in this runtime.';
                        } else {
                            activateButton.addEventListener('click', async () => {
                                const idempotencyKey = operationId('profile-switch');
                                const result = await runAction(`Activate ${id}`, idempotencyKey, () =>
                                    invoke('lifecycle:switchProfile', [id, idempotencyKey]));
                                if (result) {
                                    await Promise.all([renderProfiles(), render()]);
                                }
                            });
                        }
                        actions.append(exportButton, activateButton);
                        row.append(copy, actions);
                        profileList.appendChild(row);
                    }
                } catch (error) {
                    profileList.replaceChildren();
                    const failure = document.createElement('p');
                    failure.className = 'product-profile-empty';
                    failure.textContent = 'Profiles are unavailable in this runtime.';
                    profileList.appendChild(failure);
                }
            };
            if (profileCreate) {
                profileCreate.disabled = !commandAvailable('lifecycle:createProfileFromCurrent');
                profileCreate.title = profileCreate.disabled
                    ? 'Profile capture is unavailable in this runtime.'
                    : '';
                profileCreate.addEventListener('click', async () => {
                    const id = window.prompt?.('Profile ID (letters, numbers, dash, underscore or dot)');
                    if (!id) return;
                    try {
                        await invoke('lifecycle:createProfileFromCurrent', [id]);
                        await renderProfiles();
                    } catch (error) {
                        notifications.notify({
                            title: 'Save profile',
                            message: lifecycleMessage(error),
                            tone: 'error'
                        });
                    }
                });
            }
            if (profileImport && profileImportFile) {
                profileImport.disabled = !commandAvailable('lifecycle:importProfileLockfile');
                profileImport.title = profileImport.disabled
                    ? 'Profile import requires the transactional Rust runtime.'
                    : '';
                profileImport.addEventListener('click', () => profileImportFile.click());
                profileImportFile.addEventListener('change', async () => {
                    const [file] = profileImportFile.files || [];
                    profileImportFile.value = '';
                    if (!file) return;
                    if (file.size > 4 * 1024 * 1024) {
                        notifications.notify({
                            title: 'Import profile',
                            message: 'The lockfile exceeds the 4 MiB safety limit.',
                            tone: 'error'
                        });
                        return;
                    }
                    try {
                        await invoke('lifecycle:importProfileLockfile', [await file.text()]);
                        await renderProfiles();
                        notifications.notify({
                            title: 'Import profile',
                            message: 'The canonical lockfile was validated and saved.',
                            tone: 'success'
                        });
                    } catch (error) {
                        notifications.notify({
                            title: 'Import profile',
                            message: 'The lockfile is invalid, non-canonical or from a newer schema.',
                            tone: 'error'
                        });
                    }
                });
            }
            const progressHost = root.parentNode && typeof document.createElement === 'function'
                ? document.createElement('div')
                : null;
            if (progressHost) {
                progressHost.className = 'product-live-operation';
                progressHost.setAttribute('aria-live', 'polite');
                root.parentNode.appendChild(progressHost);
            }
            const lifecycleMessage = error => {
                const code = String(error?.message || error || '');
                if (code.includes('external_modification')) {
                    return 'The operation stopped because installed files changed outside Deltamod.';
                }
                if (code.includes('recovery_unavailable')) {
                    return 'The exact recovery artifact is unavailable. Reinstall from the original archive.';
                }
                if (code.includes('source_identity_mismatch')) {
                    return 'The selected archive belongs to a different mod.';
                }
                if (code.includes('archive_invalid')) {
                    return 'The selected archive is not a valid Deltamod package.';
                }
                if (code.includes('installation_busy')) {
                    return 'Another filesystem operation is already running for this installation.';
                }
                if (code.includes('archive_hash_missing')) {
                    return 'A legacy mod has no exact archive hash yet. Reinstall it before saving a reproducible profile.';
                }
                if (code.includes('game_unknown')) {
                    return 'Select a supported game before saving this profile.';
                }
                return 'The operation could not be completed safely. No unverified changes were committed.';
            };
            const runAction = async (title, operationIdValue, action) => {
                notifications.notify({ title, message: 'Operation started.', tone: 'info' });
                let settled = false;
                const polling = operationIdValue && progressHost
                    ? (async () => {
                        while (!settled) {
                            try {
                                const status = await invoke(
                                    'lifecycle:getOperationStatus',
                                    [operationIdValue]
                                );
                                if (status?.progress) {
                                    progressHost.replaceChildren(ProductUI.OperationProgress(
                                        status.progress,
                                        { document, title }
                                    ));
                                }
                            } catch (_error) {
                                // The operation result remains authoritative; polling is advisory.
                            }
                            await new Promise(resolve => window.setTimeout(resolve, 180));
                        }
                    })()
                    : Promise.resolve();
                try {
                    const result = await action();
                    if (!result?.cancelled) {
                        notifications.notify({
                            title,
                            message: 'Operation completed and verified.',
                            tone: 'success'
                        });
                    }
                    return result;
                } catch (error) {
                    notifications.notify({
                        title,
                        message: lifecycleMessage(error),
                        tone: 'error'
                    });
                    return null;
                } finally {
                    settled = true;
                    await polling;
                    progressHost?.replaceChildren();
                }
            };
            const adapter = fixturePreview
                ? ProductUI.InstalledModsAdapters.fixture(
                    contractsV1Fixture,
                    fixtureUiOverlay
                )
                : ProductUI.InstalledModsAdapters.lifecycleLive(invoke);
            let sequence = 0;
            const operationId = action => {
                sequence += 1;
                const random = globalThis.crypto?.randomUUID?.();
                return random || `${action}-${Date.now()}-${sequence}`;
            };
            const render = async () => {
                root.setAttribute('aria-busy', 'true');
                const model = await adapter.load();
                if (!root.isConnected) return;
                ProductUI.renderInstalledModsV2(root, model, {
                    locale: document.documentElement.lang || 'en',
                    onCopyDiagnostics: copyDiagnostics,
                    onRestore: async journal => {
                        if (typeof window.confirm !== 'function' || !window.confirm(
                            'Restore the latest working state for this package library? Current verified changes will be backed up first.'
                        )) return;
                        const id = operationId('restore');
                        const result = await runAction('Restore last working state', id, () => invoke(
                            'lifecycle:restoreLastWorkingState',
                            [journal.installationId, id]
                        ));
                        if (result) await render();
                    },
                    onOpenFolder: mod => invoke('openModFolder', [mod.folder]),
                    onUpdate: async mod => {
                        const id = operationId('update');
                        const result = await runAction(`Update ${mod.name}`, id, () => invoke(
                            'lifecycle:updateMod',
                            [mod.installationId, mod.id, id]
                        ));
                        if (result && !result.cancelled) await render();
                    },
                    onVerify: async mod => {
                        const result = await runAction(`Verify ${mod.name}`, null, () => invoke(
                            'lifecycle:verifyMod',
                            [mod.installationId, mod.id]
                        ));
                        if (result) await render();
                    },
                    onRepair: async mod => {
                        const id = operationId('repair');
                        const result = await runAction(`Repair ${mod.name}`, id, () => invoke(
                            'lifecycle:repairMod',
                            [mod.installationId, mod.id, id]
                        ));
                        if (result) await render();
                    },
                    onUninstall: async mod => {
                        if (typeof window.confirm !== 'function' || !window.confirm(
                            `Uninstall ${mod.name}? Externally changed files will be preserved and the operation will stop.`
                        )) return;
                        const id = operationId('uninstall');
                        const result = await runAction(`Uninstall ${mod.name}`, id, () => invoke(
                            'lifecycle:uninstallMod',
                            [mod.installationId, mod.id, id]
                        ));
                        if (result) await render();
                    }
                });
            };
            await render();
            await renderProfiles();
        } catch (error) {
            if (root.isConnected) renderFallbackError(root, error);
        }
    }

    start();
})();
