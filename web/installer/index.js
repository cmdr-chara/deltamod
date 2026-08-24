// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

(() => {
    'use strict';

    const backend = window.deltamodBackend;
    const form = document.getElementById('installer-form');
    const directory = document.getElementById('install-directory');
    const installButton = document.getElementById('installer-install');
    const targetSummary = document.getElementById('installer-target');
    const progress = document.getElementById('installer-progress');
    const progressFill = document.getElementById('installer-progress-fill');
    const progressTrack = progress.querySelector('.progress-track');
    const phase = document.getElementById('installer-phase');
    const percent = document.getElementById('installer-percent');
    const detail = document.getElementById('installer-detail');
    const status = document.getElementById('installer-status');
    const actions = document.getElementById('installer-actions');
    const launchButton = document.getElementById('installer-launch');
    const doneButton = document.getElementById('installer-done');
    const version = document.getElementById('installer-version');
    const heading = document.getElementById('installer-heading');
    const lede = document.querySelector('.installer-lede');
    const footerState = document.getElementById('installer-footer-state') || { textContent: '' };

    let installedDirectory = '';
    let installing = false;

    function setStatus(message, isError = false) {
        status.textContent = message;
        status.dataset.error = isError ? 'true' : 'false';
    }

    function setProgress(value, nextPhase = phase.textContent, nextDetail = '') {
        const clamped = Math.max(0, Math.min(1, Number(value) || 0));
        const display = Math.round(clamped * 100);
        progressFill.style.width = `${display}%`;
        progressTrack.setAttribute('aria-valuenow', String(display));
        phase.textContent = nextPhase;
        percent.textContent = `${String(display).padStart(2, '0')}%`;
        detail.textContent = nextDetail;
        footerState.textContent = nextPhase;
    }

    function setBusy(busy) {
        installing = busy;
        directory.disabled = busy;
        installButton.disabled = busy;
        installButton.textContent = busy ? 'Installing…' : 'Install';
        document.body.dataset.installerState = busy ? 'installing' : 'idle';
    }

    function setScreenCopy(nextState) {
        if (nextState === 'installing') {
            heading.textContent = 'Installing Deltamod Community';
            lede.textContent = 'Downloading and preparing the app. You can leave this window open.';
            return;
        }
        if (nextState === 'ready') {
            heading.textContent = 'Deltamod Community is ready';
            lede.textContent = 'Installation completed successfully.';
            return;
        }
        heading.textContent = 'Install Deltamod Community';
        lede.textContent = 'Choose a folder, then Deltamod will handle the rest.';
    }

    async function closeSetup() {
        try {
            await backend?.invoke('installerQuit', []);
        } catch {
            window.close();
        }
    }

    document.getElementById('installer-minimize').addEventListener('click', () => {
        backend?.invoke('installerMinimize', []).catch(() => {});
    });
    document.getElementById('installer-close').addEventListener('click', closeSetup);
    doneButton.addEventListener('click', closeSetup);
    directory.addEventListener('input', () => {
        directory.title = directory.value;
    });

    launchButton.addEventListener('click', async () => {
        launchButton.disabled = true;
        try {
            await backend.invoke('installerLaunch', [installedDirectory]);
            await closeSetup();
        } catch (error) {
            launchButton.disabled = false;
            setStatus(error?.message || 'Deltamod could not be launched.', true);
        }
    });

    backend?.on?.('installer-progress', payload => {
        if (!payload || typeof payload !== 'object') return;
        setProgress(payload.progress, payload.phase || 'Working', payload.detail || '');
    });

    form.addEventListener('submit', async event => {
        event.preventDefault();
        if (installing) return;
        const target = directory.value.trim();
        if (!target) {
            setStatus('Choose an installation folder to continue.', true);
            directory.focus();
            return;
        }
        if (!backend?.invoke) {
            setStatus('The standalone setup is only available inside the Deltamod shell.', true);
            return;
        }

        installedDirectory = target;
        setBusy(true);
        setScreenCopy('installing');
        form.hidden = true;
        targetSummary.hidden = false;
        targetSummary.textContent = `Installing to ${target}`;
        targetSummary.title = target;
        progress.hidden = false;
        actions.hidden = true;
        setStatus('');
        setProgress(0.08, 'Connecting', 'Contacting the Deltamod release server');
        try {
            await backend.invoke('installerInstall', [target]);
            form.hidden = true;
            actions.hidden = false;
            document.body.dataset.installerState = 'ready';
            setScreenCopy('ready');
            targetSummary.textContent = `Installed in ${target}`;
            setStatus('');
            setProgress(1, 'Ready', 'Installation complete');
            progress.hidden = true;
            footerState.textContent = 'Installation complete';
        } catch (error) {
            setBusy(false);
            document.body.dataset.installerState = 'error';
            setScreenCopy('idle');
            form.hidden = false;
            targetSummary.hidden = true;
            progress.hidden = true;
            setStatus(error?.message || 'The installation could not be completed.', true);
            footerState.textContent = 'Setup needs attention';
        }
    });

    (async () => {
        try {
            const info = await backend?.invoke?.('installerInfo', []);
            if (info?.defaultInstallDir) directory.value = info.defaultInstallDir;
            if (info?.defaultInstallDir) directory.title = info.defaultInstallDir;
            if (info?.version) version.textContent = `Version ${info.version}`;
        } catch {
            version.textContent = 'Standalone setup';
        }
    })();
})();
