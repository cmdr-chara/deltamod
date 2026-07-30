// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const { loadProvenance } = require('./lib/g3mtool-provenance');

const root = path.resolve(__dirname, '..');
const provenance = loadProvenance(root);
const output = path.resolve(
    process.argv[2] || path.join(root, 'dist', `G3MTool-${provenance.releaseTag}-source.zip`)
);
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-g3mtool-source-'));
const checkout = path.join(temporary, 'source');

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

try {
    fs.mkdirSync(path.dirname(output), { recursive: true });
    git(['init', '--quiet', checkout]);
    git(['remote', 'add', 'origin', provenance.sourceUrl], checkout);
    git(['fetch', '--quiet', '--depth', '1', 'origin', provenance.sourceRevision], checkout);
    const fetched = git(['rev-parse', 'FETCH_HEAD'], checkout);
    if (fetched.toLowerCase() !== provenance.sourceRevision.toLowerCase()) {
        throw new Error(`G3MTool source revision mismatch: expected ${provenance.sourceRevision}, received ${fetched}.`);
    }
    git([
        'archive',
        '--format=zip',
        `--prefix=G3MTool-${provenance.releaseTag}/`,
        `--output=${output}`,
        'FETCH_HEAD'
    ], checkout);
    console.log(`Archived exact G3MTool corresponding source to ${output}`);
} finally {
    fs.rmSync(temporary, { recursive: true, force: true });
}
