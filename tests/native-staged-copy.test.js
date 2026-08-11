// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { runNativeStagedCopy, _protocol } = require('../node/storage/NativeStagedCopy');
const { copyDirectoryAtomicFallback } = require('../node/storage/StagedCopy');

const roots = [];
function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-native-copy-'));
    roots.push(root);
    return root;
}
function debugBinary() {
    return path.join(__dirname, '..', 'native', 'target', 'debug', process.platform === 'win32' ? 'deltamod-copy-worker.exe' : 'deltamod-copy-worker');
}
function nativeOptions(root, extra = {}) {
    return {
        source: path.join(root, 'source'),
        destination: path.join(root, 'destination'),
        operationId: 'native-test',
        retries: 1,
        availableBytes: null,
        sidecarPath: debugBinary(),
        ...extra
    };
}

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

it('strictly validates native copy events', () => {
    const state = {
        source: path.resolve('source'), destination: path.resolve('destination'), entries: [], paths: new Set(),
        entryFiles: 0, entryBytes: 0, inventory: null, completed: 0, commit: false, done: false,
        workerError: null, eventCount: 0
    };
    expect(_protocol.parseEvent('{"type":"entry","entryType":"file","relative":"file","size":4}', state)).toBeNull();
    expect(_protocol.parseEvent(`{"type":"inventory","sourceRoot":${JSON.stringify(state.source)},"fileCount":1,"totalBytes":4}`, state)).toBeNull();
    expect(_protocol.parseEvent('{"type":"progress","completed":4,"total":4,"currentItem":"file"}', state)).toMatchObject({ phase: 'copy' });
    expect(() => _protocol.parseEvent('{"type":"done","extra":true}', state)).toThrow(/completion/i);
    expect(() => _protocol.validateRelative('../escape')).toThrow(/relative path/i);
});

it('runs the debug worker with API-compatible inventory and progress', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source', 'nested'), { recursive: true });
    fs.writeFileSync(path.join(root, 'source', 'nested', 'file'), 'native');
    const progress = [];
    const result = await runNativeStagedCopy(nativeOptions(root, { onProgress: event => progress.push(event) }));
    expect(result).toMatchObject({ fileCount: 1, totalBytes: 6 });
    expect(result.entries).toEqual(expect.arrayContaining([
        expect.objectContaining({ type: 'directory', relative: 'nested' }),
        expect.objectContaining({ type: 'file', relative: path.join('nested', 'file'), size: 6 })
    ]));
    expect(progress.at(-1)).toMatchObject({ operationId: 'native-test', phase: 'commit', completed: 6, total: 6 });
    expect(fs.readFileSync(path.join(root, 'destination', 'nested', 'file'), 'utf8')).toBe('native');
});

it('kills the worker on abort and removes staging', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    fs.writeFileSync(path.join(root, 'source', 'large'), Buffer.alloc(16 * 1024 * 1024, 1));
    const controller = new AbortController();
    const promise = runNativeStagedCopy(nativeOptions(root, {
        signal: controller.signal,
        onProgress: event => { if (event.phase === 'copy') controller.abort(); }
    }));
    await expect(promise).rejects.toMatchObject({ code: 'COPY_CANCELLED' });
    expect(fs.existsSync(path.join(root, 'destination'))).toBe(false);
    expect(fs.readdirSync(root).some(name => name.includes('.importing-'))).toBe(false);
});

it('finishes an atomic commit when cancellation arrives after the commit boundary', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    fs.writeFileSync(path.join(root, 'source', 'file'), 'committed');
    const controller = new AbortController();
    const result = await runNativeStagedCopy(nativeOptions(root, {
        signal: controller.signal,
        onProgress: event => { if (event.phase === 'commit') controller.abort(); }
    }));

    expect(result).toMatchObject({ fileCount: 1, totalBytes: 9 });
    expect(fs.readFileSync(path.join(root, 'destination', 'file'), 'utf8')).toBe('committed');
});

it('fails closed for a present non-worker binary', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    await expect(runNativeStagedCopy(nativeOptions(root, { sidecarPath: process.execPath })))
        .rejects.toMatchObject({ code: 'COPY_NATIVE_FAILED' });
});

it('fails closed for an explicitly missing worker', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    await expect(runNativeStagedCopy(nativeOptions(root, { sidecarPath: path.join(root, 'missing-worker') })))
        .rejects.toMatchObject({ code: 'COPY_NATIVE_FAILED' });
});

it('rejects existing destinations and source hardlinks', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    fs.writeFileSync(path.join(root, 'source', 'one'), 'data');
    fs.writeFileSync(path.join(root, 'destination'), 'existing');
    await expect(runNativeStagedCopy(nativeOptions(root))).rejects.toMatchObject({ code: 'DESTINATION_EXISTS' });
    fs.rmSync(path.join(root, 'destination'));
    fs.linkSync(path.join(root, 'source', 'one'), path.join(root, 'source', 'two'));
    await expect(runNativeStagedCopy(nativeOptions(root))).rejects.toMatchObject({ code: 'SOURCE_LINK_BLOCKED' });
});

it('detects deterministic source mutation during copying', async () => {
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    const sourceFile = path.join(root, 'source', 'large');
    fs.writeFileSync(sourceFile, Buffer.alloc(4 * 1024 * 1024, 1));
    let mutated = false;
    await expect(runNativeStagedCopy(nativeOptions(root, {
        onProgress: event => {
            if (!mutated && event.phase === 'copy') {
                mutated = true;
                fs.appendFileSync(sourceFile, 'changed');
            }
        }
    }))).rejects.toMatchObject({ code: 'SOURCE_CHANGED' });
    expect(fs.existsSync(path.join(root, 'destination'))).toBe(false);
});

it('keeps the named fallback and rejects dangling destinations', async () => {
    if (process.platform === 'win32') return;
    const root = fixture();
    fs.mkdirSync(path.join(root, 'source'));
    fs.symlinkSync(path.join(root, 'missing'), path.join(root, 'destination'));
    await expect(copyDirectoryAtomicFallback(nativeOptions(root, { sidecarPath: undefined })))
        .rejects.toMatchObject({ code: 'DESTINATION_EXISTS' });
});

it('fallback detects destination-parent replacement before commit', async () => {
    const root = fixture();
    const parent = path.join(root, 'parent');
    fs.mkdirSync(path.join(parent, 'source'), { recursive: true });
    fs.writeFileSync(path.join(parent, 'source', 'file'), 'data');
    let replaced = false;
    await expect(copyDirectoryAtomicFallback({
        source: path.join(parent, 'source'),
        destination: path.join(parent, 'destination'),
        operationId: 'fallback-parent-test',
        onProgress: event => {
            if (!replaced && event.phase === 'commit') {
                replaced = true;
                fs.renameSync(parent, `${parent}-old`);
                fs.mkdirSync(parent);
            }
        }
    })).rejects.toMatchObject({ code: 'DESTINATION_PARENT_CHANGED' });
    expect(fs.existsSync(path.join(parent, 'destination'))).toBe(false);
});
