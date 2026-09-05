/* Original, local utility glyphs. No remote font request or ligature dependency. */
(function (root, factory) {
    const icons = factory();
    if (typeof module === 'object' && module.exports) module.exports = icons;
    else {
        root.DeltamodIcons = icons;
        const pending = new Set();
        let frame = null;
        const schedule = node => {
            if (node?.nodeType !== 1) return;
            pending.add(node);
            if (frame !== null) return;
            frame = requestAnimationFrame(() => {
                frame = null;
                for (const element of pending) if (element.isConnected) icons.hydrate(element);
                pending.clear();
            });
        };
        const start = () => {
            icons.hydrate(document.body);
            const observer = new MutationObserver(records => {
                for (const record of records) {
                    if (record.type === 'characterData') schedule(record.target.parentElement);
                    else for (const node of record.addedNodes) {
                        if (node.nodeType === 3) schedule(node.parentElement);
                        else if (node.namespaceURI !== 'http://www.w3.org/2000/svg') schedule(node);
                    }
                }
            });
            observer.observe(document.body, { childList: true, subtree: true, characterData: true });
            root.addEventListener('pagehide', () => { observer.disconnect(); if (frame !== null) cancelAnimationFrame(frame); pending.clear(); }, { once: true });
        };
        if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
        else start();
    }
})(typeof window === 'undefined' ? globalThis : window, function () {
    'use strict';
    const shapes = {
        add: '<path d="M12 5v14M5 12h14"/>',
        check: '<path d="m5 12 4 4L19 6"/>',
        close: '<path d="m6 6 12 12M18 6 6 18"/>',
        right: '<path d="m9 5 7 7-7 7"/>',
        left: '<path d="m15 5-7 7 7 7"/>',
        arrow: '<path d="M4 12h15m-6-6 6 6-6 6"/>',
        external: '<path d="M14 4h6v6m0-6L10 14M10 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-5"/>',
        folder: '<path d="M3 7V5h7l2 2h9v13H3zM3 10h18"/>',
        download: '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
        upload: '<path d="M12 16V4m-5 5 5-5 5 5M4 16v5h16v-5"/>',
        trash: '<path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7"/>',
        warning: '<path d="m12 3 10 18H2zM12 9v5m0 3v.1"/>',
        info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6m0-10v.1"/>',
        person: '<circle cx="12" cy="8" r="4"/><path d="M4 21v-2a8 8 0 0 1 16 0v2"/>',
        heart: '<path d="M12 21 3.5 12.5a5.5 5.5 0 0 1 8.5-7 5.5 5.5 0 0 1 8.5 7z"/>',
        search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
        zoomIn: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5M7 10.5h7m-3.5-3.5v7"/>',
        zoomOut: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5M7 10.5h7"/>',
        fit: '<path d="M3 9V3h6m6 0h6v6M3 15v6h6m6 0h6v-6"/>',
        globe: '<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>',
        settings: '<path d="m9 3-1 3-3 1-2 4 2 3v4l4 3 3-1 3 1 4-3v-4l2-3-2-4-3-1-1-3z"/><circle cx="12" cy="12" r="3"/>',
        tune: '<path d="M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6"/>',
        palette: '<path d="M21 11a9 9 0 1 0-9 10h2a2 2 0 0 0 1-3.7c-1-.6-1-2.3 1-2.3h2a3 3 0 0 0 3-4z"/><path d="M7 9h.1M11 6h.1M16 8h.1M6 14h.1"/>',
        terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3m6 1h4"/>',
        code: '<path d="m7 6-5 6 5 6m10-12 5 6-5 6m-3-15-4 18"/>',
        database: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 4 16 4 16 0V5M4 12c0 4 16 4 16 0"/>',
        game: '<path d="M7 7h10c3 0 4 4 4 9 0 4-4 4-6-1H9c-2 5-6 5-6 1 0-5 1-9 4-9zM7 9v5m-2-2.5h4m6-.5h.1m3 2h.1"/>',
        key: '<circle cx="7" cy="8" r="4"/><path d="m10 11 10 10m-6-6 3-3m0 6 3-3"/>',
        comment: '<path d="M4 3h16v14H9l-5 4zM8 7h8m-8 4h5"/>',
        refresh: '<path d="M20 9a8 8 0 1 0 0 7M20 3v6h-6"/>',
        clock: '<circle cx="12" cy="12" r="9"/><path d="M12 6v6l4 2"/>',
        box: '<path d="m12 2 9 5v10l-9 5-9-5V7zm-9 5 9 5 9-5M12 12v10M7 4l10 6"/>',
        network: '<circle cx="12" cy="5" r="3"/><circle cx="5" cy="19" r="3"/><circle cx="19" cy="19" r="3"/><path d="m10 8-4 8m8-8 4 8M8 19h8"/>',
        minus: '<circle cx="12" cy="12" r="9"/><path d="M7 12h10"/>',
        balance: '<path d="M12 3v18M5 21h14M4 7h16M5 7l-3 7h6zm14 0-3 7h6z"/>',
        more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
        shield: '<path d="m12 2 8 3v7c0 5-8 10-8 10S4 17 4 12V5zM8 12l3 3 5-6"/>',
        triangle: '<path d="m12 3 10 18H2z"/>'
    };
    const aliases = {
        add_box: 'add', create_new_folder: 'folder', folder_open: 'folder', folder_eye: 'folder',
        drive_folder_upload: 'upload', bottom_panel_open: 'upload', bottom_panel_close: 'download', cloud_download: 'download',
        arrow_circle_right: 'arrow', forward: 'arrow', chevron_right: 'right', chevron_left: 'left', open_in_new: 'external',
        delete: 'trash', delete_forever: 'trash', bomb: 'warning', error: 'warning', error_outline: 'warning',
        account_circle: 'person', attribution: 'person', account_tree: 'network', public: 'globe', language: 'globe',
        cycle: 'refresh', sync: 'refresh', sync_arrow_up: 'refresh', update: 'refresh', acute: 'clock', history: 'clock',
        deployed_code: 'box', inventory: 'box', inventory_2: 'box', hub: 'network', gamepad_up: 'game', stadia_controller: 'game',
        mood_heart: 'heart', favorite: 'heart', sentiment_very_satisfied: 'heart', thumb_up: 'heart',
        do_not_disturb_on: 'minus', block: 'minus', close: 'close', cancel: 'close',
        fit_screen: 'fit', zoom_in: 'zoomIn', zoom_out: 'zoomOut', build: 'settings',
        change_history: 'triangle', fiber_new: 'add', verified: 'shield', security: 'shield',
        join: 'network', check_circle: 'check', download_done: 'check', help: 'info', help_outline: 'info',
        game_stick_left: 'game', game_stick_right: 'game', square_circle: 'game'
    };
    function markup(name, size = '1em') {
        const glyph = shapes[aliases[name] || name] || shapes.more;
        const sizes = { small: '14px', medium: '18px', large: '24px' };
        const dimension = sizes[size] || (/^\d+(?:\.\d+)?(?:px|em|rem)$/.test(String(size)) ? String(size) : '1em');
        return `<svg class="dm-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false" style="width:${dimension};height:${dimension}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${glyph}</svg>`;
    }
    function hydrate(scope) {
        if (!scope?.querySelectorAll) return;
        const nodes = [...scope.querySelectorAll('.material-symbols-outlined, button[data-icon]')];
        if (scope.matches?.('.material-symbols-outlined, button[data-icon]')) nodes.unshift(scope);
        for (const element of nodes) {
            if (element.matches('button[data-icon]')) {
                if (!element.querySelector('.dm-icon')) element.insertAdjacentHTML('afterbegin', markup(element.dataset.icon, '18px'));
            } else {
                const name = element.textContent.trim();
                if (!name) continue;
                element.dataset.glyph = name;
                element.setAttribute('aria-hidden', 'true');
                element.innerHTML = markup(name);
            }
        }
    }
    return Object.freeze({ markup, hydrate });
});
