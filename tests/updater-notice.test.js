// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const mount = require('../web/modules/updater-notice');
const fs = require('node:fs');
const path = require('node:path');

class Element {
    constructor(tag) { this.tag = tag; this.children = []; this.attributes = {}; this.listeners = new Map(); this.hidden = false; }
    append(...items) { for (const item of items) { item.parent = this; this.children.push(item); } }
    setAttribute(name, value) { this.attributes[name] = value; }
    removeAttribute(name) { delete this.attributes[name]; if (name === 'value') delete this.value; }
    addEventListener(type, listener) { this.listeners.set(type, listener); }
    removeEventListener(type, listener) { if (this.listeners.get(type) === listener) this.listeners.delete(type); }
    remove() { this.parent.children = this.parent.children.filter(item => item !== this); }
    click() { return this.listeners.get('click')?.(); }
}
function fixture() {
    const document = { body: new Element('body'), createElement: tag => new Element(tag) };
    const callbacks = {};
    const api = {
        cancel: vi.fn(async () => true), status: vi.fn(async () => ({ state: 'installing' })),
        onStatus: handler => { callbacks.status = handler; return vi.fn(() => { callbacks.status = null; }); },
        onProgress: handler => { callbacks.progress = handler; return vi.fn(() => { callbacks.progress = null; }); }
    };
    const handle = mount(document, api);
    const panel = document.body.children[0];
    const [title, message, progress, cancel, dismiss] = panel.children;
    return { document, api, callbacks, handle, panel, title, message, progress, cancel, dismiss };
}
const deferred = () => { let resolve; let reject; const promise = new Promise((r, j) => { resolve = r; reject = j; }); return { promise, resolve, reject }; };

describe('native updater progress and cancellation', () => {
    it('starts hidden and displays accessible, bounded progress only for the updater', () => {
        const f = fixture();
        expect(f.panel.hidden).toBe(true);
        expect(f.message.attributes.role).toBe('status');
        f.callbacks.status({ state: 'downloading' });
        expect(f.panel.hidden).toBe(false);
        expect(f.cancel.disabled).toBe(false);
        f.callbacks.progress({ operationId: 'unrelated', total: 10, completed: 8 });
        expect(f.progress.value).toBeUndefined();
        f.callbacks.progress({ operationId: 'community-update', total: 10, completed: 11 });
        expect(f.progress.value).toBe(10);
        f.callbacks.progress({ operationId: 'community-update', total: null, completed: 12 });
        expect(f.progress.value).toBeUndefined();
        f.callbacks.progress({ operationId: 'community-update', total: Infinity, completed: 12 });
        expect(f.progress.value).toBeUndefined();
    });
    it('permits one cancellation request and waits for native acknowledgement', async () => {
        const f = fixture(); const response = deferred(); f.api.cancel.mockReturnValue(response.promise);
        f.callbacks.status({ state: 'downloading' });
        const request = f.cancel.click(); await f.cancel.click();
        expect(f.api.cancel).toHaveBeenCalledTimes(1);
        expect(f.cancel.disabled).toBe(true);
        response.resolve(true); await request;
        expect(f.message.textContent).toBe('Cancelling the download...');
        expect(f.dismiss.hidden).toBe(true);
        f.callbacks.status({ state: 'cancelled' });
        expect(f.cancel.hidden).toBe(true);
        expect(f.dismiss.hidden).toBe(false);
        f.dismiss.click(); expect(f.panel.hidden).toBe(true);
    });
    it('does not turn an installation into cancellation on a late acknowledgement', async () => {
        const f = fixture(); const response = deferred(); f.api.cancel.mockReturnValue(response.promise);
        f.callbacks.status({ state: 'downloading' }); const request = f.cancel.click();
        f.callbacks.status({ state: 'installing' }); response.resolve(true); await request;
        expect(f.message.textContent).toContain('Installing');
        expect(f.cancel.disabled).toBe(true);
    });
    it('queries native state when the cancellation commit race is lost', async () => {
        const f = fixture(); f.api.cancel.mockResolvedValue(false);
        f.callbacks.status({ state: 'downloading' }); await f.cancel.click();
        expect(f.api.status).toHaveBeenCalledOnce();
        expect(f.message.textContent).toContain('Installing');
        expect(f.cancel.disabled).toBe(true);
    });
    it('ignores a stale cancellation result after another download begins', async () => {
        const f = fixture(); const response = deferred(); f.api.cancel.mockReturnValue(response.promise);
        f.callbacks.status({ state: 'downloading' }); const request = f.cancel.click();
        f.callbacks.status({ state: 'cancelled' }); f.callbacks.status({ state: 'downloading' });
        response.resolve(true); await request;
        expect(f.message.textContent).toBe('Downloading the signed update.');
        expect(f.cancel.disabled).toBe(false);
    });
    it('keeps cancellation retryable after a transport rejection', async () => {
        const f = fixture(); f.api.cancel.mockRejectedValue(new Error('IPC unavailable'));
        f.callbacks.status({ state: 'downloading' }); await f.cancel.click();
        expect(f.cancel.disabled).toBe(false);
        expect(f.message.textContent).toContain('Try again');
    });
    it('uses literal bounded error text and preserves terminal cancellation', () => {
        const f = fixture();
        f.handle.failure('<img src=x onerror=alert(1)>' + 'a'.repeat(1024));
        expect(f.message.textContent).toHaveLength(512);
        expect(f.message.children).toHaveLength(0);
        f.callbacks.status({ state: 'cancelled' }); f.handle.failure('generic invoke error');
        expect(f.message.textContent).toContain('Download cancelled');
    });
    it('disposes subscriptions and ignores outstanding cancellation promises', async () => {
        const f = fixture(); const response = deferred(); f.api.cancel.mockReturnValue(response.promise);
        f.callbacks.status({ state: 'downloading' }); const request = f.cancel.click();
        f.handle.dispose(); f.handle.dispose(); response.resolve(true); await request;
        expect(f.callbacks.status).toBeNull(); expect(f.callbacks.progress).toBeNull();
        expect(f.document.body.children).toHaveLength(0);
        expect(f.cancel.listeners.size).toBe(0);
    });
    it('integrates the notice and native cancel bridge in the production renderer', () => {
        const root = path.join(__dirname, '..');
        const html = fs.readFileSync(path.join(root, 'web/index.html'), 'utf8');
        const adapter = fs.readFileSync(path.join(root, 'web/tauri-adapter.js'), 'utf8');
        const channel = fs.readFileSync(path.join(root, 'src-tauri/src/channels/updater.rs'), 'utf8');
        expect(html).toContain('modules/updater-notice.js');
        expect(html).toContain('modules/updater-notice.css');
        expect(adapter).toContain("cancel: () => invoke('cancel-update')");
        expect(channel.indexOf('state.updater_control.cancel()')).toBeLessThan(channel.indexOf('state.updater.try_lock()'));
    });
});
