/* Copyright © 2026 Deltamod contributors. Licensed under the EUPL 1.2. */
(() => {
    'use strict';
    let tail = Promise.resolve();
    const owns = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
    const sound = source => {
        window.deltamodBackend.invoke('getUniqueFlag', ['SFX']).then(enabled => {
            if (enabled) new Audio(source).play().catch(() => {});
        }).catch(() => {});
    };
    async function settle(button) {
        if (owns(button, 'resolveWith')) return button.resolveWith;
        if (owns(button, 'rejectWith')) throw button.rejectWith;
        return typeof button.onClick === 'function' ? button.onClick() : undefined;
    }
    async function display(title, message, suppliedButtons) {
        const buttons = suppliedButtons?.length ? suppliedButtons : [{ text: 'OK' }];
        let separate = false;
        try { separate = localStorage.getItem('alertAlignment') === 'Separate'; } catch (_) {}
        if (separate && window.deltamodBackend.isCommandAvailable('htmlAlert_outwin')) {
            const index = await window.deltamodBackend.invoke('htmlAlert_outwin', [title, message, buttons]);
            if (!Number.isInteger(index) || index < 0 || index >= buttons.length) throw new Error('Dialog dismissed');
            return buttons[index];
        }
        const overlay = document.querySelector('.alertMain');
        const panel = overlay?.querySelector('.alertMsg');
        if (!panel) throw new Error('Dialog host is unavailable');
        const previous = document.activeElement;
        const background = [...document.querySelectorAll('body > .viewport, body > .sidebar, body > .language-wheel-toggle, body > .gamebanana-account')];
        const inertBefore = background.map(element => element.inert);
        background.forEach(element => { element.inert = true; });
        panel.replaceChildren();
        panel.setAttribute('role', 'alertdialog');
        panel.setAttribute('aria-modal', 'true');
        panel.setAttribute('aria-labelledby', 'workspace-alert-title');
        panel.setAttribute('aria-describedby', 'workspace-alert-description');
        panel.tabIndex = -1;
        const heading = document.createElement('h1');
        heading.id = 'workspace-alert-title'; heading.textContent = String(title);
        const copy = document.createElement('p');
        copy.id = 'workspace-alert-description'; copy.textContent = String(message); copy.style.whiteSpace = 'pre-line';
        const actions = document.createElement('div'); actions.className = 'alertButtons';
        panel.append(heading, copy, actions);
        overlay.hidden = false; overlay.style.display = 'flex';
        const choice = await new Promise(resolve => {
            let chosen = false;
            const choose = index => {
                if (chosen) return;
                chosen = true;
                actions.querySelectorAll('button').forEach(button => { button.disabled = true; });
                panel.removeEventListener('keydown', keydown);
                resolve(index);
            };
            const elements = buttons.map((button, index) => {
                const element = document.createElement('button');
                element.type = 'button'; element.textContent = String(button.text || 'OK');
                if (index > 0) element.className = 'secondary-action';
                element.onclick = () => choose(index); actions.appendChild(element); return element;
            });
            const cancel = buttons.findIndex(button => owns(button, 'rejectWith') || button.resolveWith === false || /^(cancel|no|later)$/i.test(button.text));
            function keydown(event) {
                if (event.key === 'Escape' && cancel >= 0) { event.preventDefault(); choose(cancel); }
                if (event.key !== 'Tab') return;
                const index = elements.indexOf(document.activeElement);
                if ((event.shiftKey && index <= 0) || (!event.shiftKey && (index < 0 || index === elements.length - 1))) {
                    event.preventDefault(); elements[event.shiftKey ? elements.length - 1 : 0].focus();
                }
            }
            panel.addEventListener('keydown', keydown);
            window.Localization?.applyKnownText(panel);
            (cancel >= 0 ? elements[cancel] : elements[0]).focus({ preventScroll: true });
            sound('audio/htmlalert.mp3');
        });
        overlay.hidden = true; overlay.style.display = 'none'; panel.replaceChildren();
        background.forEach((element, index) => { element.inert = inertBefore[index]; });
        const restoreFocus = () => {
            if (previous?.isConnected && !previous.closest('[inert]') && !previous.disabled) {
                previous.focus({ preventScroll: true });
            }
        };
        restoreFocus();
        // The caller may re-enable its initiating control in the settled promise's
        // finally block. Retry after that microtask, but never steal a newer focus.
        requestAnimationFrame(() => {
            if (overlay.hidden && document.activeElement === document.body) restoreFocus();
        });
        sound('audio/booow.mp3');
        return buttons[choice];
    }
    function show(title, message, buttons) {
        const request = tail.then(() => display(title, message, buttons));
        tail = request.catch(() => {});
        return request.then(settle);
    }
    window.DeltamodDialogs = Object.freeze({ show });
})();
