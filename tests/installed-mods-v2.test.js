// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ProductUI = require('../web/modules/product-ui.js');

const repositoryRoot = path.join(__dirname, '..');
const contractsFixturePath = path.join(
    repositoryRoot,
    'src-tauri',
    'crates',
    'product-contracts',
    'tests',
    'fixtures',
    'contracts-v1.json'
);

class TestEvent {
    constructor(type, init = {}) {
        this.type = type;
        this.bubbles = init.bubbles !== false;
        this.key = init.key || '';
        this.shiftKey = Boolean(init.shiftKey);
        this.defaultPrevented = false;
        this.propagationStopped = false;
        this.target = init.target || null;
        this.currentTarget = null;
    }

    preventDefault() {
        this.defaultPrevented = true;
    }

    stopPropagation() {
        this.propagationStopped = true;
    }
}

class TestClassList {
    constructor(element) {
        this.element = element;
    }

    values() {
        return this.element.className.split(/\s+/).filter(Boolean);
    }

    write(values) {
        this.element.className = Array.from(new Set(values)).join(' ');
    }

    add(...tokens) {
        this.write([...this.values(), ...tokens]);
    }

    remove(...tokens) {
        const removed = new Set(tokens);
        this.write(this.values().filter(token => !removed.has(token)));
    }

    contains(token) {
        return this.values().includes(token);
    }
}

class TestNode {
    constructor(ownerDocument, nodeType) {
        this.ownerDocument = ownerDocument;
        this.nodeType = nodeType;
        this.parentNode = null;
        this.childNodes = [];
    }

    appendChild(node) {
        const child = typeof node === 'string'
            ? this.ownerDocument.createTextNode(node)
            : node;
        if (child.parentNode) child.parentNode.removeChild(child);
        child.parentNode = this;
        this.childNodes.push(child);
        return child;
    }

    append(...nodes) {
        nodes.forEach(node => this.appendChild(node));
    }

    removeChild(node) {
        const index = this.childNodes.indexOf(node);
        if (index === -1) throw new Error('Node is not a child.');
        this.childNodes.splice(index, 1);
        node.parentNode = null;
        return node;
    }

    replaceChildren(...nodes) {
        this.childNodes.forEach(child => {
            child.parentNode = null;
        });
        this.childNodes = [];
        this.append(...nodes);
    }

    remove() {
        if (this.parentNode) this.parentNode.removeChild(this);
    }

    contains(node) {
        if (node === this) return true;
        return this.childNodes.some(child => child.contains(node));
    }

    get firstChild() {
        return this.childNodes[0] || null;
    }

    get lastChild() {
        return this.childNodes[this.childNodes.length - 1] || null;
    }

    get children() {
        return this.childNodes.filter(child => child.nodeType === 1);
    }

    get textContent() {
        return this.childNodes.map(child => child.textContent).join('');
    }

    set textContent(value) {
        this.replaceChildren();
        const text = String(value ?? '');
        if (text) this.appendChild(this.ownerDocument.createTextNode(text));
    }

    get isConnected() {
        let current = this;
        while (current) {
            if (current.nodeType === 9) return true;
            current = current.parentNode;
        }
        return false;
    }
}

class TestTextNode extends TestNode {
    constructor(ownerDocument, data) {
        super(ownerDocument, 3);
        this.data = String(data);
    }

    contains(node) {
        return node === this;
    }

    get textContent() {
        return this.data;
    }

    set textContent(value) {
        this.data = String(value ?? '');
    }
}

function matchesSelector(element, selector) {
    const candidate = selector.trim();
    if (!candidate || element.nodeType !== 1) return false;

    const attributes = Array.from(candidate.matchAll(/\[([^\]=\s]+)(?:=["']([^"']*)["'])?\]/g));
    for (const match of attributes) {
        const [, name, expected] = match;
        if (!element.hasAttribute(name)) return false;
        if (expected !== undefined && element.getAttribute(name) !== expected) return false;
    }

    const withoutAttributes = candidate.replace(/\[[^\]]+\]/g, '');
    const tag = withoutAttributes.match(/^[a-z][a-z0-9-]*/i)?.[0];
    if (tag && element.tagName !== tag.toUpperCase()) return false;

    const id = withoutAttributes.match(/#([a-z0-9_-]+)/i)?.[1];
    if (id && element.id !== id) return false;

    const classes = Array.from(withoutAttributes.matchAll(/\.([a-z0-9_-]+)/gi), match => match[1]);
    return classes.every(className => element.classList.contains(className));
}

class TestElement extends TestNode {
    constructor(ownerDocument, tagName) {
        super(ownerDocument, 1);
        this.tagName = String(tagName).toUpperCase();
        this.attributes = new Map();
        this.listeners = new Map();
        this.classList = new TestClassList(this);
        this.hidden = false;
        this.disabled = false;
        this.open = false;
        this.value = '';
        this.returnValue = '';
    }

    get className() {
        return this.getAttribute('class') || '';
    }

    set className(value) {
        this.setAttribute('class', String(value || ''));
    }

    get id() {
        return this.getAttribute('id') || '';
    }

    set id(value) {
        this.setAttribute('id', String(value || ''));
    }

    setAttribute(name, value) {
        this.attributes.set(String(name), String(value));
        if (name === 'open') this.open = true;
    }

    getAttribute(name) {
        return this.attributes.has(String(name))
            ? this.attributes.get(String(name))
            : null;
    }

    hasAttribute(name) {
        return this.attributes.has(String(name));
    }

    removeAttribute(name) {
        this.attributes.delete(String(name));
        if (name === 'open') this.open = false;
    }

    addEventListener(type, listener, options = {}) {
        const listeners = this.listeners.get(type) || [];
        listeners.push({ listener, once: Boolean(options && options.once) });
        this.listeners.set(type, listeners);
    }

    removeEventListener(type, listener) {
        const listeners = this.listeners.get(type) || [];
        this.listeners.set(type, listeners.filter(entry => entry.listener !== listener));
    }

    dispatchEvent(event) {
        if (!event.target) event.target = this;
        let current = this;
        while (current && !event.propagationStopped) {
            event.currentTarget = current;
            const entries = [...(current.listeners?.get(event.type) || [])];
            entries.forEach(entry => {
                entry.listener.call(current, event);
                if (entry.once) current.removeEventListener(event.type, entry.listener);
            });
            current = event.bubbles ? current.parentNode : null;
        }

        if (
            this.tagName === 'BUTTON' &&
            event.type === 'keydown' &&
            ['Enter', ' '].includes(event.key) &&
            !event.defaultPrevented
        ) {
            this.click();
        }
        return !event.defaultPrevented;
    }

    click() {
        if (this.disabled) return;
        this.dispatchEvent(new TestEvent('click'));
    }

    focus() {
        if (!this.disabled) this.ownerDocument.activeElement = this;
    }

    showModal() {
        this.open = true;
        this.setAttribute('open', '');
    }

    close(returnValue = '') {
        if (!this.open) return;
        this.returnValue = returnValue;
        this.removeAttribute('open');
        this.dispatchEvent(new TestEvent('close', { bubbles: false }));
    }

    querySelectorAll(selector) {
        const selectors = selector.split(',').map(value => value.trim()).filter(Boolean);
        const results = [];
        const visit = node => {
            node.childNodes.forEach(child => {
                if (child.nodeType === 1) {
                    if (selectors.some(candidate => matchesSelector(child, candidate))) {
                        results.push(child);
                    }
                    visit(child);
                }
            });
        };
        visit(this);
        return results;
    }

    querySelector(selector) {
        return this.querySelectorAll(selector)[0] || null;
    }
}

class TestDocument extends TestNode {
    constructor() {
        super(null, 9);
        this.ownerDocument = this;
        this.defaultView = {
            matchMedia: () => ({ matches: false })
        };
        this.documentElement = new TestElement(this, 'html');
        this.documentElement.lang = 'en';
        this.head = new TestElement(this, 'head');
        this.body = new TestElement(this, 'body');
        this.documentElement.append(this.head, this.body);
        this.appendChild(this.documentElement);
        this.activeElement = this.body;
    }

    createElement(tagName) {
        return new TestElement(this, tagName);
    }

    createTextNode(data) {
        return new TestTextNode(this, data);
    }

    getElementById(id) {
        return this.querySelector(`#${id}`);
    }

    querySelectorAll(selector) {
        const selectors = selector.split(',').map(value => value.trim()).filter(Boolean);
        const results = [];
        if (selectors.some(candidate => matchesSelector(this.documentElement, candidate))) {
            results.push(this.documentElement);
        }
        return [...results, ...this.documentElement.querySelectorAll(selector)];
    }

    querySelector(selector) {
        return this.querySelectorAll(selector)[0] || null;
    }
}

function readFixture() {
    return JSON.parse(fs.readFileSync(contractsFixturePath, 'utf8'));
}

function createRoot(document) {
    const root = document.createElement('div');
    document.body.appendChild(root);
    return root;
}

describe('Installed Mods v2 product UI', () => {
    it('keeps keyboard focus visible in forced colors and mobile action order aligned with the DOM', () => {
        const css = fs.readFileSync(
            path.join(repositoryRoot, 'web', 'modules', 'product-ui.css'),
            'utf8'
        );
        const forcedColors = css.match(/@media \(forced-colors: active\) \{([\s\S]*)\}\s*$/)?.[1] || '';

        expect(forcedColors).toContain('.product-source-link:focus-visible');
        expect(forcedColors).toContain('outline: 2px solid Highlight !important;');
        expect(css).not.toMatch(
            /\.product-dialog-actions\s*\{[^}]*flex-direction:\s*column-reverse/s
        );
    });

    it('maps the accepted contracts-v1 fixture through the read-only adapter', async () => {
        const fixture = readFixture();
        const model = ProductUI.mapContractsV1Fixture(fixture);

        expect(model).toMatchObject({
            source: 'contracts-v1',
            schemaVersion: 1,
            status: 'ready',
            readOnly: true
        });
        expect(model.mods).toHaveLength(1);
        expect(model.mods[0]).toMatchObject({
            id: 'fixture-instance',
            installationId: 'fixture-installation',
            name: 'Fixture Mod',
            version: '1.0.0',
            archiveSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
            fileCount: 1,
            installedAtMs: 1700000000000,
            provider: {
                id: 'gamebanana',
                displayName: 'GameBanana',
                canonicalUrl: 'https://gamebanana.com/mods/1234'
            },
            verification: { state: 'healthy', checkedFiles: 1 },
            health: { state: 'healthy' },
            update: { state: 'current', availableVersion: null },
            readOnly: true
        });
        expect(model.mods[0].conflicts).toHaveLength(1);
        expect(model.operationProgress.state).toBe('succeeded');

        const adapter = ProductUI.InstalledModsAdapters.fixture(fixture);
        const snapshot = await adapter.load();
        expect(adapter.kind).toBe('fixture');
        expect(Object.isFrozen(snapshot)).toBe(true);
        expect(Object.isFrozen(snapshot.mods[0])).toBe(true);
        expect(Object.isFrozen(fixture)).toBe(false);
        await expect(ProductUI.InstalledModsAdapters.live().load()).rejects.toThrow(
            /no backend contract commands/i
        );
    });

    it('maps the live legacy catalogue without fabricating lifecycle evidence', async () => {
        const calls = [];
        const adapter = ProductUI.InstalledModsAdapters.legacyLive(async (channel, args) => {
            calls.push([channel, args]);
            return {
                modList: [{
                    uid: 'live-mod',
                    folder: 'live-folder',
                    packageID: 'community.live.mod',
                    name: 'Live Mod',
                    version: '2.4.0',
                    game: 'deltarune',
                    archiveSha256: 'A'.repeat(64),
                    sourceUrl: 'javascript:alert(1)',
                    gamebanana: { supports: false }
                }],
                errors: [{ mod: 'broken-mod', reason: 'Unreadable metadata' }]
            };
        });
        const model = await adapter.load();

        expect(calls).toEqual([['getModList', []]]);
        expect(adapter.kind).toBe('legacy-live');
        expect(Object.isFrozen(model)).toBe(true);
        expect(model).toMatchObject({
            source: 'legacy-live',
            status: 'ready',
            readOnly: true,
            libraryWarning: '1 installed record could not be read.'
        });
        expect(model.mods[0]).toMatchObject({
            id: 'live-mod',
            folder: 'live-folder',
            installationId: 'deltarune',
            modId: 'community.live.mod',
            name: 'Live Mod',
            version: '2.4.0',
            archiveSha256: 'a'.repeat(64),
            fileCount: null,
            installedAtMs: null,
            update: { state: 'unknown', availableVersion: null },
            verification: { state: 'unverified' },
            health: { state: 'unknown' },
            provider: {
                id: 'unknown',
                displayName: 'Source not recorded',
                canonicalUrl: null
            },
            readOnly: true
        });
        expect(model.operationRecords).toEqual([]);
        expect(model.lifecycleJournals).toEqual([]);

        await expect(ProductUI.InstalledModsAdapters.legacyLive(async () => ({
            modList: [],
            errors: []
        })).load()).resolves.toMatchObject({ status: 'empty', mods: [] });
        await expect(ProductUI.InstalledModsAdapters.legacyLive(async () => ({
            modList: [],
            errors: [{ mod: 'broken' }]
        })).load()).resolves.toMatchObject({
            status: 'error',
            productError: { code: 'installed_mod_scan_failed' }
        });
        await expect(ProductUI.InstalledModsAdapters.legacyLive(async () => null).load())
            .rejects.toThrow(/invalid response/i);

        const document = new TestDocument();
        const root = createRoot(document);
        const onOpenFolder = vi.fn();
        ProductUI.renderInstalledModsV2(root, model, {
            document,
            locale: 'en',
            onOpenFolder
        });
        const openFolder = root.querySelector('[data-lifecycle-action="open-folder"]');
        expect(openFolder.disabled).toBe(false);
        expect(openFolder.getAttribute('aria-disabled')).toBe('false');
        openFolder.click();
        expect(onOpenFolder).toHaveBeenCalledWith(model.mods[0]);
        for (const action of ['update', 'verify', 'uninstall']) {
            expect(root.querySelector(`[data-lifecycle-action="${action}"]`).disabled).toBe(true);
        }
    });

    it('maps the authoritative lifecycle catalogue and enables accepted actions', async () => {
        const fixture = readFixture();
        const calls = [];
        const adapter = ProductUI.InstalledModsAdapters.lifecycleLive(async (channel, args) => {
            calls.push([channel, args]);
            return {
                installedMods: [fixture.installedMod],
                verificationResults: [fixture.verificationResult],
                gameHealthReports: [fixture.gameHealthReport],
                conflictReports: [],
                operationRecords: [],
                lifecycleJournals: [],
                foldersByInstanceId: { 'fixture-instance': 'fixture-folder' },
                errors: [],
                runtime: { shell: 'tauri', platform: 'windows', version: '2.0.13' }
            };
        });
        const model = await adapter.load();
        expect(calls).toEqual([['lifecycle:getInstalledMods', []]]);
        expect(model).toMatchObject({ source: 'contracts-v1', readOnly: false, status: 'ready' });
        expect(model.mods[0]).toMatchObject({
            id: 'fixture-instance',
            folder: 'fixture-folder',
            readOnly: false
        });

        const document = new TestDocument();
        const root = createRoot(document);
        const onVerify = vi.fn();
        const onUninstall = vi.fn();
        ProductUI.renderInstalledModsV2(root, model, { document, onVerify, onUninstall });
        const verify = root.querySelector('[data-lifecycle-action="verify"]');
        const uninstall = root.querySelector('[data-lifecycle-action="uninstall"]');
        expect(verify.disabled).toBe(false);
        expect(uninstall.disabled).toBe(false);
        verify.click();
        uninstall.click();
        expect(onVerify).toHaveBeenCalledWith(model.mods[0]);
        expect(onUninstall).toHaveBeenCalledWith(model.mods[0]);
    });

    it('uses live local data by default and fixtures only behind an explicit flag', async () => {
        const pageScript = fs.readFileSync(
            path.join(repositoryRoot, 'web', 'views', 'allmods-v2', 'index.js'),
            'utf8'
        );
        const invokes = [];
        let renderedModel = null;
        const root = { isConnected: true, setAttribute() {} };
        const ProductUIStub = {
            NotificationCenter() {
                return { element: {}, notify() {} };
            },
            InstalledModsAdapters: {
                fixture() {
                    throw new Error('production must not load fixtures');
                },
                lifecycleLive(invoke) {
                    return {
                        load: async () => {
                            await invoke('lifecycle:getInstalledMods', []);
                            return { source: 'contracts-v1', status: 'empty', mods: [] };
                        }
                    };
                }
            },
            renderInstalledModsV2(_root, model) {
                renderedModel = model;
            }
        };
        const window = {
            DeltamodProductUI: ProductUIStub,
            deltamodBackend: {
                invoke: async (channel, args) => {
                    invokes.push([channel, args]);
                    return { installedMods: [], errors: [] };
                }
            }
        };
        const document = {
            documentElement: { lang: 'en' },
            getElementById: id => id === 'installed-mods-v2-root' ? root : null,
            querySelector: () => ({})
        };

        vm.runInNewContext(pageScript, { window, document, Promise, Error });
        await new Promise(resolve => setImmediate(resolve));

        expect(invokes).toEqual([['lifecycle:getInstalledMods', []]]);
        expect(renderedModel).toMatchObject({ source: 'contracts-v1', status: 'empty' });
        expect(pageScript).toContain('__DELTAMOD_PRODUCT_UI_FIXTURES__ === true');
        expect(pageScript).toContain('InstalledModsAdapters.lifecycleLive');

        let fixtureUsed = false;
        const fixtureWindow = {
            __DELTAMOD_PRODUCT_UI_FIXTURES__: true,
            DeltamodProductUI: {
                NotificationCenter() {
                    return { element: {}, notify() {} };
                },
                InstalledModsAdapters: {
                    fixture() {
                        fixtureUsed = true;
                        return { load: async () => ({ status: 'empty', mods: [] }) };
                    },
                    lifecycleLive() {
                        throw new Error('fixture preview must not use live data');
                    }
                },
                renderInstalledModsV2() {}
            }
        };
        vm.runInNewContext(pageScript, {
            window: fixtureWindow,
            document,
            Promise,
            Error
        });
        await new Promise(resolve => setImmediate(resolve));
        expect(fixtureUsed).toBe(true);
    });

    it('renders untrusted contract text literally and rejects unsafe source URLs', () => {
        const fixture = readFixture();
        fixture.installedMod.displayName = '<img src=x onerror="globalThis.__installedModsXss = true">';
        fixture.installedMod.provider.canonicalUrl = 'javascript:globalThis.__installedModsXss=true';
        fixture.providerDescriptor.displayName = '<script>globalThis.__installedModsXss=true</script>';
        fixture.conflictReport.conflicts[0].path = '<svg onload="globalThis.__installedModsXss=true">';
        const document = new TestDocument();
        const root = createRoot(document);

        const model = ProductUI.mapContractsV1Fixture(fixture, {
            artworkByInstanceId: { 'fixture-instance': './%252e%252e/secret.png' }
        });
        ProductUI.renderInstalledModsV2(root, model, { document, locale: 'en' });

        expect(root.textContent).toContain('<img src=x onerror=');
        expect(root.textContent).toContain('<script>globalThis.__installedModsXss=true</script>');
        expect(root.textContent).toContain('<svg onload=');
        expect(root.querySelector('script')).toBeNull();
        expect(root.querySelector('[onerror]')).toBeNull();
        expect(globalThis.__installedModsXss).toBeUndefined();
        expect(root.querySelector('img').getAttribute('src')).toBe('./img/mod-placeholder.png');

        const source = root.querySelector('[data-testid="mod-source-link"]');
        expect(source.tagName).toBe('SPAN');
        expect(source.hasAttribute('href')).toBe(false);
    });

    it('opens conflict review from the keyboard, traps focus, and restores the trigger', () => {
        const document = new TestDocument();
        const root = createRoot(document);
        const model = ProductUI.mapContractsV1Fixture(readFixture());
        ProductUI.renderInstalledModsV2(root, model, { document, locale: 'en' });

        const trigger = root.querySelector('[data-testid="review-conflicts"]');
        const dialog = root.querySelector('dialog');
        trigger.focus();
        trigger.dispatchEvent(new TestEvent('keydown', { key: 'Enter' }));

        expect(dialog.open).toBe(true);
        expect(document.activeElement.textContent).toBe('Close');

        const tab = new TestEvent('keydown', { key: 'Tab' });
        dialog.dispatchEvent(tab);
        expect(tab.defaultPrevented).toBe(true);
        expect(document.activeElement.textContent).toBe('Close');

        const escape = new TestEvent('keydown', { key: 'Escape' });
        dialog.dispatchEvent(escape);
        expect(escape.defaultPrevented).toBe(true);
        expect(dialog.open).toBe(false);
        expect(document.activeElement).toBe(trigger);
    });

    it('renders empty, error, offline, skeleton, progress, and conflict states', () => {
        const document = new TestDocument();
        const root = createRoot(document);

        ProductUI.renderInstalledModsV2(root, { status: 'empty', mods: [] }, { document });
        expect(root.querySelector('[data-state="empty"]').textContent).toContain('No installed mods');

        ProductUI.renderInstalledModsV2(
            root,
            {
                status: 'error',
                mods: [],
                productError: { code: 'installation_busy' }
            },
            { document }
        );
        const error = root.querySelector('[data-state="error"]');
        expect(error.getAttribute('role')).toBe('alert');
        expect(error.textContent).toContain('Another operation is using this installation');

        ProductUI.renderInstalledModsV2(root, { status: 'offline', mods: [] }, { document });
        expect(root.querySelector('[data-state="offline"]').textContent).toContain('offline');

        ProductUI.renderInstalledModsV2(root, { status: 'loading', mods: [] }, { document });
        const skeleton = root.querySelector('[data-state="loading"]');
        expect(skeleton.getAttribute('aria-busy')).toBe('true');
        expect(skeleton.querySelectorAll('.product-skeleton-row')).toHaveLength(2);

        const fixture = readFixture();
        const progress = ProductUI.OperationProgress(fixture.operationProgress, { document });
        const progressElement = progress.querySelector('progress');
        expect(progress.getAttribute('role')).toBe('status');
        expect(progressElement.getAttribute('max')).toBe('1');
        expect(progressElement.getAttribute('value')).toBe('1');
        expect(progress.textContent).toContain('Complete · 1 of 1');

        const conflict = ProductUI.ConflictDialog(fixture.conflictReport, { document });
        document.body.appendChild(conflict.element);
        expect(conflict.element.textContent).toContain('mods/fixture.dat');
        expect(conflict.element.textContent).toContain('Different content');
    });

    it('shows update, warning, provider, health, metadata, and disabled lifecycle actions', () => {
        const document = new TestDocument();
        const root = createRoot(document);
        const model = ProductUI.mapContractsV1Fixture(readFixture(), {
            availableVersionByInstanceId: { 'fixture-instance': '1.1.0' }
        });
        ProductUI.renderInstalledModsV2(root, model, { document, locale: 'en' });

        expect(root.textContent).toContain('Fixture Mod');
        expect(root.textContent).toContain('Version1.0.0');
        expect(root.textContent).toContain('GameBanana');
        expect(root.textContent).toContain('Verified healthy');
        expect(root.textContent).toContain('Game healthy');
        expect(root.textContent).toContain('1 file');
        expect(root.textContent).toContain('a'.repeat(64));
        expect(root.textContent).toContain('1.1.0 available');
        expect(root.querySelector('[data-state="warning"]')).not.toBeNull();
        expect(root.querySelector('[data-state="conflict"]')).not.toBeNull();
        expect(root.querySelector('time').getAttribute('datetime')).toBe(
            '2023-11-14T22:13:20.000Z'
        );

        const lifecycleActions = root.querySelectorAll('[data-lifecycle-action]');
        expect(lifecycleActions).toHaveLength(5);
        lifecycleActions.forEach(button => {
            expect(button.tagName).toBe('BUTTON');
            expect(button.disabled).toBe(true);
            expect(button.getAttribute('aria-disabled')).toBe('true');
        });

        const source = root.querySelector('[data-testid="mod-source-link"]');
        expect(source.tagName).toBe('A');
        expect(source.getAttribute('href')).toBe('https://gamebanana.com/mods/1234');
        expect(source.getAttribute('rel')).toBe('noopener noreferrer');
    });

    it('puts a factual game-health summary before operations and grades severe states', () => {
        const document = new TestDocument();
        const root = createRoot(document);
        const fixture = readFixture();
        fixture.gameHealthReport.state = 'interrupted_operation';
        fixture.gameHealthReport.interruptedOperations = ['operation-1'];
        fixture.gameHealthReport.lifecycleOwnedFiles = 12;
        fixture.gameHealthReport.unknownModifiedFiles = 2;
        const model = ProductUI.mapContractsV1Fixture(fixture);

        ProductUI.renderInstalledModsV2(root, model, { document, locale: 'en' });

        const view = root.querySelector('.product-installed-mods-view');
        const summary = root.querySelector('.product-health-summary');
        const operation = root.querySelector('.product-operation-progress');
        expect(summary.getAttribute('aria-labelledby')).toBeTruthy();
        expect(summary.getAttribute('data-health-state')).toBe('interrupted_operation');
        expect(summary.textContent).toContain('Interrupted operation');
        expect(summary.querySelector('[data-testid="health-managed-files"]').textContent).toBe('12');
        expect(summary.querySelector('[data-testid="health-external-changes"]').textContent).toBe('2');
        expect(summary.querySelector('[data-testid="health-interrupted-operations"]').textContent).toBe('1');
        expect(view.childNodes.indexOf(summary)).toBeLessThan(view.childNodes.indexOf(operation));

        const expectedTones = new Map([
            ['modified_as_expected', 'is-success'],
            ['external_changes_detected', 'is-warning'],
            ['missing_files', 'is-danger'],
            ['conflicting_ownership', 'is-danger'],
            ['repair_available', 'is-warning']
        ]);
        expectedTones.forEach((tone, state) => {
            const badge = ProductUI.HealthBadge({ state }, { document });
            expect(badge.classList.contains(tone)).toBe(true);
        });

        const mixedSummary = ProductUI.GameHealthSummary([
            { state: 'unknown' },
            { state: 'missing_files' }
        ], { document });
        expect(mixedSummary.getAttribute('data-health-state')).toBe('missing_files');
    });

    it('shows persistent operation, recovery, and allowlisted support diagnostics', () => {
        const document = new TestDocument();
        const root = createRoot(document);
        const model = ProductUI.mapContractsV1Fixture(readFixture(), {
            runtime: {
                version: '2.0.13',
                shell: 'electron',
                platform: 'windows',
                token: 'must-never-serialize',
                homePath: 'C:\\Users\\Sensitive'
            }
        });

        ProductUI.renderInstalledModsV2(root, model, { document, locale: 'en' });

        expect(root.querySelector('.product-operations-center').textContent)
            .toContain('InstallComplete');
        const recovery = root.querySelector('.product-recovery-center');
        expect(recovery.textContent).toContain('Last working state');
        expect(recovery.querySelector('button').disabled).toBe(true);
        const diagnostics = root.querySelector('.product-diagnostics-panel');
        expect(diagnostics.textContent).toContain('1 mods · 1 operations · 1 recovery generations');
        expect(JSON.stringify(model.diagnostics)).not.toContain('must-never-serialize');
        expect(JSON.stringify(model.diagnostics)).not.toContain('Sensitive');
        expect(model.diagnostics.providers).toEqual(['gamebanana']);

        const onCopy = vi.fn();
        const copyPanel = ProductUI.DiagnosticsPanel(model.diagnostics, { document, onCopy });
        copyPanel.querySelector('button').click();
        expect(onCopy).toHaveBeenCalledOnce();
        expect(JSON.parse(onCopy.mock.calls[0][0])).toEqual(model.diagnostics);
    });

    it('provides focus-safe confirmation and text-safe toast primitives', () => {
        const document = new TestDocument();
        const opener = document.createElement('button');
        opener.textContent = 'Remove mod';
        document.body.appendChild(opener);
        const onConfirm = vi.fn();
        const confirmation = ProductUI.DangerConfirmation({
            document,
            title: 'Remove Fixture Mod?',
            message: 'This action cannot be undone.',
            requiredText: 'Fixture Mod',
            onConfirm
        });
        document.body.appendChild(confirmation.element);

        opener.focus();
        confirmation.open(opener);
        expect(confirmation.element.open).toBe(true);
        expect(document.activeElement).toBe(confirmation.confirmationInput);
        expect(confirmation.confirmButton.disabled).toBe(true);

        confirmation.confirmationInput.value = 'Fixture Mod';
        confirmation.confirmationInput.dispatchEvent(new TestEvent('input'));
        expect(confirmation.confirmButton.disabled).toBe(false);
        confirmation.confirmButton.click();
        expect(onConfirm).toHaveBeenCalledOnce();
        expect(confirmation.element.open).toBe(false);
        expect(document.activeElement).toBe(opener);

        confirmation.open(opener);
        expect(confirmation.confirmationInput.value).toBe('');
        expect(confirmation.confirmButton.disabled).toBe(true);
        confirmation.close();

        const notifications = ProductUI.NotificationCenter({ document });
        document.body.appendChild(notifications.element);
        const toast = notifications.notify({
            title: '<b>Safe title</b>',
            message: '<img src=x onerror=alert(1)>',
            tone: 'warning'
        });
        expect(toast.element.textContent).toContain('<b>Safe title</b>');
        expect(toast.element.querySelector('img')).toBeNull();
        toast.closeButton.focus();
        toast.closeButton.dispatchEvent(new TestEvent('keydown', { key: 'Enter' }));
        expect(toast.element.parentNode).toBeNull();

        const css = fs.readFileSync(
            path.join(repositoryRoot, 'web', 'modules', 'product-ui.css'),
            'utf8'
        );
        expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    });

    it('fails closed for unknown contract schema versions', () => {
        const fixture = readFixture();
        fixture.installedMod.schemaVersion = 2;
        expect(() => ProductUI.mapContractsV1Fixture(fixture)).toThrow(
            /unsupported installed_mod schema version/i
        );
    });
});
