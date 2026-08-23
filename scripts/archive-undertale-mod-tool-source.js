// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const sevenZip = require('7zip-min');
const { loadProvenance } = require('./lib/undertale-mod-tool-provenance');

const root = path.resolve(__dirname, '..');
const provenance = loadProvenance(root);
const output = path.resolve(
    process.argv[2] || path.join(root, 'dist', `UndertaleModTool-${provenance.releaseTag}-source.zip`)
);
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-utmt-source-'));
const checkout = path.join(temporary, 'checkout');
const archiveRoot = path.join(temporary, `UndertaleModTool-${provenance.releaseTag}`);

function git(args, cwd = temporary) {
    const result = spawnSync('git', args, {
        cwd,
        encoding: 'utf8',
        timeout: 120_000,
        windowsHide: true
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(result.stderr || `git ${args[0]} failed.`);
    return result.stdout.trim();
}

function removeTree(target) {
    fs.rmSync(target, {
        recursive: true,
        force: true,
        maxRetries: 8,
        retryDelay: 125
    });
}

function stageWorkingTree(source, destination) {
    fs.cpSync(source, destination, {
        recursive: true,
        force: true,
        filter(sourcePath) {
            return path.basename(sourcePath) !== '.git';
        }
    });
}

function cleanupTemporary() {
    try {
        removeTree(temporary);
    } catch (error) {
        // The runner is ephemeral. Cleanup must never invalidate an archive that
        // was already produced and verified successfully.
        console.warn(`Could not remove temporary UndertaleModTool source checkout: ${error.message}`);
    }
}

async function main() {
    fs.mkdirSync(path.dirname(output), { recursive: true });
    git(['init', '--quiet', checkout]);
    git(['remote', 'add', 'origin', provenance.sourceUrl], checkout);
    git(['fetch', '--quiet', '--depth', '1', 'origin', provenance.sourceRevision], checkout);

    if (git(['rev-parse', 'FETCH_HEAD'], checkout) !== provenance.sourceRevision) {
        throw new Error('UndertaleModTool source revision mismatch.');
    }

    const releaseRevision = git(
        ['ls-remote', 'origin', `refs/tags/${provenance.releaseTag}`],
        checkout
    ).split(/\s+/)[0];
    if (releaseRevision !== provenance.releaseRevision) {
        throw new Error('UndertaleModTool release tag revision mismatch.');
    }

    git(['checkout', '--quiet', '--detach', 'FETCH_HEAD'], checkout);
    git(['-c', 'protocol.file.allow=never', 'submodule', 'update', '--init', '--depth', '1'], checkout);

    // Copy the checked-out working tree into a clean staging directory instead
    // of deleting .git directories in-place. The previous in-place deletion
    // intermittently and then deterministically failed with ENOTEMPTY on hosted
    // Linux runners, blocking otherwise valid stable releases.
    stageWorkingTree(checkout, archiveRoot);
    await sevenZip.pack(archiveRoot, output);
    console.log(`Archived exact UndertaleModTool corresponding source to ${output}`);
}

main()
    .catch(error => {
        console.error(error.message);
        process.exitCode = 1;
    })
    .finally(cleanupTemporary);
