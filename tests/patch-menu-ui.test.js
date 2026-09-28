// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-02.
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');

describe('Patch menu UI', () => {
    it('groups sorting and actions in one compact toolbar', () => {
        const markup = read('web/views/main/index.html');

        expect(markup).toContain('<header class="patch-toolbar">');
        expect(markup).toContain('class="patch-sort-control"');
        expect(markup).toContain('class="patch-actions"');
        expect(markup).toContain('id="importModBtn"');
        expect(markup).toContain('id="par"');
    });

    it('uses a themed accessible toggle instead of the native checkbox chrome', () => {
        const css = read('web/views/main/main.css');
        const script = read('web/views/main/index.js');

        expect(css).toContain('.patch-toggle-track::after');
        expect(css).toContain('.patch-toggle input:focus-visible + .patch-toggle-track');
        expect(css).not.toContain('border: 6px solid white');
        expect(script).toContain("toggleLabel.className = 'patch-toggle'");
        expect(script).toContain("enabled.setAttribute('aria-label', `Enable ${mod.name}`)");
        expect(script).toContain("modRow.classList.toggle('is-enabled', isEnabled)");
    });

    it('keeps Patch menu motion minimal and respects reduced-motion preferences', () => {
        const css = read('web/views/main/main.css');

        expect(css).not.toMatch(/\bscale(?:3d|X|Y)?\s*\(/i);
        expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*\.patch-toggle-track::after[\s\S]*transition: none/);
    });
});

describe('patch failure visibility', () => {
    it('shows the native rejection instead of launching vanilla or hiding the failure', async () => {
        const { runInNewContext } = require('node:vm');
        const source = read('web/views/main/index.js');
        const action = source.slice(source.indexOf('async function patchAndRun()'), source.indexOf('window.currentPageStack.patchAndRun ='));
        const calls = [];
        const alerts = [];
        const reason = 'This build cannot safely run G3MTool. The mod was not applied and the game was not launched.';
        const launchButton = { disabled: false };
        let synced = 0;
        const patchAndRun = runInNewContext('let launching = false; ' + action + '; patchAndRun', {
            pageIsActive: () => true, listReady: true, pendingWrites: new Set(),
            hasUnsavedEnabledVariants: () => false, launchButton, noMergeMods: [],
            pageTable: { querySelectorAll: () => [{ id: 'modcheck-one', checked: true }] },
            page: async name => calls.push(['page', name]),
            window: { deltamodBackend: { invoke: async channel => {
                calls.push(['invoke', channel]);
                throw new Error(reason);
            } } },
            console: { log() {} }, t: (_, fallback) => fallback,
            htmlAlert: async (...args) => alerts.push(args),
            syncLaunchButton: () => { synced++; }
        });
        await patchAndRun();
        await patchAndRun();
        expect(alerts.map(a => a[1])).toEqual([reason, reason]);
        expect(calls.filter(([kind]) => kind === 'invoke').map(([, channel]) => channel)).toEqual(['patchAndRun', 'patchAndRun']);
        expect(synced).toBe(2);
    });
    it('keeps restoration ownership in the native runtime and rejects failed IPC', () => {
        const handler = read('src-tauri/src/channels/patching.rs');
        const failed = handler.slice(handler.lastIndexOf('Err(error) =>'));
        expect(failed).toContain('Err(error.to_string())');
        expect(handler).not.toContain('state.patching.restore()');
    });
});
