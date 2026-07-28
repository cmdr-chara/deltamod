const fs = require('fs');
const os = require('os');
const path = require('path');
const { inspectSourceTree, copyDirectoryAtomic } = require('../node/storage/StagedCopy');

const roots = [];
function temporaryRoot() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-copy-'));
    roots.push(root);
    return root;
}

afterEach(() => {
    while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true });
});

it('copies a directory through staging and reports byte progress', async () => {
    const root = temporaryRoot();
    const source = path.join(root, 'source');
    const destination = path.join(root, 'destination');
    fs.mkdirSync(path.join(source, 'nested'), { recursive: true });
    fs.writeFileSync(path.join(source, 'nested', 'data.win'), '1234567890');
    const progress = [];

    const result = await copyDirectoryAtomic({
        source,
        destination,
        onProgress: event => progress.push(event)
    });

    expect(result.totalBytes).toBe(10);
    expect(fs.readFileSync(path.join(destination, 'nested', 'data.win'), 'utf8')).toBe('1234567890');
    expect(progress.at(-1)).toMatchObject({ phase: 'commit', completed: 10, total: 10 });
    expect(fs.readdirSync(root).some(name => name.includes('.importing-'))).toBe(false);
});

it('does not leave destination or staging data when cancelled', async () => {
    const root = temporaryRoot();
    const source = path.join(root, 'source');
    const destination = path.join(root, 'destination');
    fs.mkdirSync(source);
    fs.writeFileSync(path.join(source, 'data.win'), 'data');
    const controller = new AbortController();
    controller.abort();

    await expect(copyDirectoryAtomic({
        source,
        destination,
        signal: controller.signal
    })).rejects.toMatchObject({ code: 'COPY_CANCELLED' });
    expect(fs.existsSync(destination)).toBe(false);
    expect(fs.readdirSync(root).some(name => name.includes('.importing-'))).toBe(false);
});

it('rejects linked source entries', async () => {
    if (process.platform === 'win32') return;
    const root = temporaryRoot();
    const source = path.join(root, 'source');
    fs.mkdirSync(source);
    fs.symlinkSync(root, path.join(source, 'escape'));
    await expect(inspectSourceTree(source)).rejects.toMatchObject({ code: 'SOURCE_LINK_BLOCKED' });
});

it('fails before copying when destination capacity is insufficient', async () => {
    const root = temporaryRoot();
    const source = path.join(root, 'source');
    const destination = path.join(root, 'destination');
    fs.mkdirSync(source);
    fs.writeFileSync(path.join(source, 'data.win'), 'game-data');

    await expect(copyDirectoryAtomic({
        source,
        destination,
        availableBytes: 1
    })).rejects.toMatchObject({
        code: 'INSUFFICIENT_SPACE',
        details: { availableBytes: 1 }
    });
    expect(fs.existsSync(destination)).toBe(false);
});
