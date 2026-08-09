const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const roots = [];

afterEach(() => { while (roots.length) fs.rmSync(roots.pop(), { recursive: true, force: true }); });

function binary() {
    return path.join(__dirname, '..', 'native', 'target', 'debug', process.platform === 'win32'
        ? 'deltamod-patch-transaction-worker.exe' : 'deltamod-patch-transaction-worker');
}

function fixture() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-transaction-'));
    roots.push(root);
    const game = path.join(root, 'game');
    fs.mkdirSync(path.join(game, 'data'), { recursive: true });
    fs.writeFileSync(path.join(game, 'data', 'target.txt'), 'original');
    return { root, game };
}

function run(request) {
    const result = spawnSync(binary(), [], { input: `${JSON.stringify(request)}\n`, encoding: 'utf8', windowsHide: true, shell: false });
    expect(result.status).toBe(0);
    expect(result.stderr).toBe('');
    return JSON.parse(result.stdout);
}

function journal(transactionId = '123-456', operations = []) {
    return { schemaVersion: 1, transactionId: transactionId, state: 'patching', operations };
}

describe('native patch transaction worker', () => {
    it('durably backs up and reverse-restores a target', () => {
        const data = fixture();
        expect(run({ action: 'backup', game_root: data.game, journal: journal(), target: 'data/target.txt' })).toEqual({ ok: true });
        const saved = JSON.parse(fs.readFileSync(path.join(data.game, '.deltamod-community-patch-journal.json')));
        expect(saved.operations).toEqual([{ type: 'restore', target: 'data/target.txt', backup: 'data/target.txt', state: 'applied' }]);
        fs.writeFileSync(path.join(data.game, 'data', 'target.txt'), 'patched');
        expect(run({ action: 'restore', game_root: data.game, journal: saved })).toEqual({ ok: true });
        expect(fs.readFileSync(path.join(data.game, 'data', 'target.txt'), 'utf8')).toBe('original');
        expect(fs.existsSync(path.join(data.game, '.deltamod-community-patch-journal.json'))).toBe(false);
    });

    it('rejects malformed and escaping journals without mutating the target', () => {
        const data = fixture();
        const response = run({ action: 'restore', game_root: data.game, journal: journal('../../escape', [{ type: 'remove', target: '../outside', state: 'applied' }]) });
        expect(response).toMatchObject({ ok: false, code: 'PATCH_TRANSACTION_INVALID' });
        expect(fs.readFileSync(path.join(data.game, 'data', 'target.txt'), 'utf8')).toBe('original');
    });

    it('fails closed when an applied backup is missing', () => {
        const data = fixture();
        const response = run({ action: 'restore', game_root: data.game, journal: journal('123-456', [{ type: 'restore', target: 'data/target.txt', backup: 'data/target.txt', state: 'applied' }]) });
        expect(response).toMatchObject({ ok: false, code: 'PATCH_TRANSACTION_INVALID' });
        expect(fs.readFileSync(path.join(data.game, 'data', 'target.txt'), 'utf8')).toBe('original');
    });
});
