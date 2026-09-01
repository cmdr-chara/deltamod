'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { runNativeHashWorker } = require('../../node/workers/NativeHashWorker');
const { validateExtractedTreeNative } = require('../../node/security/NativeArchiveSecurity');
const { runNativeStagedCopy } = require('../../node/storage/NativeStagedCopy');
const { validatePatchPlanNative } = require('../../node/security/NativePatchPlanValidation');

const SIDECARS = Object.freeze([
    'hash-worker',
    'security-worker',
    'copy-worker',
    'patch-plan-worker',
    'patch-transaction-worker'
]);

const HOST_TARGETS = Object.freeze({
    'win32-x64': 'x86_64-pc-windows-msvc',
    'linux-x64': 'x86_64-unknown-linux-gnu',
    'darwin-x64': 'x86_64-apple-darwin',
    'darwin-arm64': 'aarch64-apple-darwin'
});

function option(name) {
    const index = process.argv.indexOf(name);
    if (index < 0 || index + 1 >= process.argv.length) return null;
    return process.argv[index + 1];
}

function sha256(file) {
    return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function installedSidecars(installRoot) {
    const entries = fs.readdirSync(installRoot, { withFileTypes: true });
    return Object.fromEntries(SIDECARS.map(name => {
        // Tauri consumes target-suffixed staging names and installs each
        // external binary under its runtime name.
        const expected = `deltamod-${name}${process.platform === 'win32' ? '.exe' : ''}`;
        const matches = entries.filter(entry => entry.isFile() && entry.name === expected);
        if (matches.length !== 1) throw new Error(`Expected exactly one installed ${name} sidecar.`);
        const file = path.join(installRoot, expected);
        const stat = fs.lstatSync(file);
        if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1024) {
            throw new Error(`Installed ${name} sidecar is invalid.`);
        }
        if (process.platform !== 'win32') fs.accessSync(file, fs.constants.X_OK);
        return [name, file];
    }));
}

function transaction(binary, request) {
    const result = spawnSync(binary, [], {
        input: `${JSON.stringify(request)}\n`,
        encoding: 'utf8',
        windowsHide: true,
        shell: false,
        timeout: 30_000,
        maxBuffer: 16 * 1024
    });
    if (result.error || result.status !== 0 || result.stderr !== '') {
        throw new Error('Installed patch transaction sidecar failed its bounded protocol.');
    }
    const lines = result.stdout.trimEnd().split('\n');
    if (lines.length !== 1) throw new Error('Installed patch transaction returned an invalid response count.');
    const response = JSON.parse(lines[0]);
    if (response?.ok !== true || Object.keys(response).join(',') !== 'ok') {
        throw new Error('Installed patch transaction rejected its fixture operation.');
    }
    return response;
}

async function run() {
    const installRootArgument = option('--install-root');
    const evidenceArgument = option('--evidence-file');
    const targetArgument = option('--target');
    if (!installRootArgument || !evidenceArgument || !targetArgument) {
        throw new Error('Usage: run-installed-sidecar-smoke --install-root <path> --target <triple> --evidence-file <path>');
    }
    const hostTarget = HOST_TARGETS[`${process.platform}-${process.arch}`];
    if (!hostTarget || targetArgument !== hostTarget) {
        throw new Error(`Installed sidecar target ${targetArgument} does not match host ${process.platform}-${process.arch}.`);
    }
    const installRoot = fs.realpathSync(installRootArgument);
    const evidenceFile = path.resolve(evidenceArgument);
    const binaries = installedSidecars(installRoot);
    const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-installed-sidecars-'));

    try {
        const source = path.join(fixtureRoot, 'archive-extracted');
        const imported = path.join(fixtureRoot, 'imported-mod');
        const cache = path.join(fixtureRoot, 'hash-cache.json');
        const game = path.join(fixtureRoot, 'game');
        fs.mkdirSync(path.join(source, 'nested'), { recursive: true });
        fs.mkdirSync(path.join(game, 'data'), { recursive: true });
        fs.writeFileSync(path.join(source, 'nested', 'fixture.txt'), 'installed-sidecar-smoke', 'utf8');
        fs.writeFileSync(path.join(game, 'data', 'target.txt'), 'original', 'utf8');

        const tree = await validateExtractedTreeNative(source, {
            maxFiles: 10,
            maxExpandedBytes: 1024,
            maxDepth: 4
        }, { sidecarPath: binaries['security-worker'] });
        if (tree.fileCount !== 1 || tree.expandedBytes !== 23) {
            throw new Error('Installed archive-security sidecar returned unexpected inventory.');
        }

        const copyProgress = [];
        const copy = await runNativeStagedCopy({
            source,
            destination: imported,
            operationId: 'installed-sidecar-smoke',
            retries: 1,
            availableBytes: null,
            sidecarPath: binaries['copy-worker'],
            onProgress: event => copyProgress.push(event)
        });
        if (copy.fileCount !== 1
            || fs.readFileSync(path.join(imported, 'nested', 'fixture.txt'), 'utf8') !== 'installed-sidecar-smoke'
            || copyProgress.at(-1)?.phase !== 'commit') {
            throw new Error('Installed atomic-copy sidecar did not publish the fixture import.');
        }

        const hashProgress = [];
        const hash = await runNativeHashWorker({
            root: imported,
            cachePath: cache,
            operationId: 'installed-sidecar-smoke',
            sidecarPath: binaries['hash-worker']
        }, event => hashProgress.push(event));
        const cacheRecord = JSON.parse(fs.readFileSync(cache, 'utf8'));
        if (hash.fileCount !== 1
            || hashProgress.at(-1)?.done !== true
            || Object.keys(cacheRecord.entries).length !== 1) {
            throw new Error('Installed hash sidecar did not persist exact fixture evidence.');
        }

        const patchPlan = await validatePatchPlanNative({
            schemaVersion: 1,
            gameRoot: game,
            platform: process.platform,
            patches: []
        }, { sidecarPath: binaries['patch-plan-worker'] });
        if (patchPlan.operationCount !== 0 || patchPlan.patchCount !== 0) {
            throw new Error('Installed patch-plan sidecar returned an unexpected empty plan.');
        }

        const journal = {
            schemaVersion: 1,
            transactionId: '123-456',
            state: 'patching',
            operations: []
        };
        transaction(binaries['patch-transaction-worker'], {
            action: 'backup',
            game_root: game,
            journal,
            target: 'data/target.txt'
        });
        const savedJournal = JSON.parse(fs.readFileSync(path.join(game, '.deltamod-community-patch-journal.json'), 'utf8'));
        fs.writeFileSync(path.join(game, 'data', 'target.txt'), 'patched', 'utf8');
        transaction(binaries['patch-transaction-worker'], {
            action: 'restore',
            game_root: game,
            journal: savedJournal
        });
        if (fs.readFileSync(path.join(game, 'data', 'target.txt'), 'utf8') !== 'original'
            || fs.existsSync(path.join(game, '.deltamod-community-patch-journal.json'))) {
            throw new Error('Installed patch-transaction sidecar did not restore exactly.');
        }

        const evidence = {
            schemaVersion: 1,
            status: 'passed',
            ok: true,
            platform: process.platform,
            architecture: process.arch,
            target: hostTarget,
            checks: {
                fiveInstalledSidecars: true,
                archiveTreeValidated: true,
                importPublishedAtomically: true,
                importedContentHashed: true,
                patchPlanValidated: true,
                patchRollbackRestoredExactly: true
            },
            sidecars: Object.fromEntries(SIDECARS.map(name => [name, { sha256: sha256(binaries[name]) }]))
        };
        fs.mkdirSync(path.dirname(evidenceFile), { recursive: true });
        fs.writeFileSync(evidenceFile, `${JSON.stringify(evidence, null, 2)}\n`, { encoding: 'utf8', flag: 'w' });
        process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
    } finally {
        fs.rmSync(fixtureRoot, { recursive: true, force: true });
    }
}

run().catch(error => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
});
