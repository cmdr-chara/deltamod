// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.
// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { Transform } = require('stream');
const { pipeline } = require('stream/promises');
const sevenZip = require('7zip-min');
const { installPath, loadProvenance, targetForCurrentPlatform, treeDigest, verifyInstallation } = require('./lib/undertale-mod-tool-provenance');

const root = path.resolve(__dirname, '..');
const allowedHosts = new Set(['github.com', 'objects.githubusercontent.com', 'release-assets.githubusercontent.com']);

function targets(provenance) {
    const explicit = process.argv.slice(2).filter(value => !value.startsWith('-'));
    if (process.argv.includes('--all')) return Object.keys(provenance.artifacts);
    if (explicit.length > 1) throw new Error('Acquire at most one UndertaleModTool target.');
    const result = explicit.length ? explicit : [targetForCurrentPlatform()];
    if (result.some(value => !provenance.artifacts[value])) throw new Error(`Unknown UndertaleModTool target: ${result[0]}`);
    return result;
}

async function download(url, destination, expectedSize, redirects = 0) {
    if (redirects > 5) throw new Error('Too many UndertaleModTool download redirects.');
    const parsed = new URL(url);
    if (parsed.protocol !== 'https:' || parsed.username || parsed.password || !allowedHosts.has(parsed.hostname)) {
        throw new Error(`UndertaleModTool download URL is not approved: ${parsed}`);
    }
    const response = await new Promise((resolve, reject) => {
        const request = require('https').get(parsed, { headers: { 'User-Agent': 'Deltamod-Community-CI/1' }, timeout: 30_000 }, resolve);
        request.on('timeout', () => request.destroy(new Error('UndertaleModTool download timed out.')));
        request.on('error', reject);
    });
    if ([301, 302, 303, 307, 308].includes(response.statusCode)) {
        response.resume();
        if (!response.headers.location) throw new Error('UndertaleModTool download returned an empty redirect.');
        return download(new URL(response.headers.location, parsed).toString(), destination, expectedSize, redirects + 1);
    }
    if (response.statusCode !== 200) {
        response.resume();
        throw new Error(`UndertaleModTool download returned HTTP ${response.statusCode}.`);
    }
    let received = 0;
    const limiter = new Transform({
        transform(chunk, encoding, callback) {
            received += chunk.length;
            callback(received > expectedSize ? new Error('UndertaleModTool archive exceeded its pinned size.') : null, chunk);
        }
    });
    await pipeline(response, limiter, fs.createWriteStream(destination, { flags: 'wx', mode: 0o600 }));
    if (received !== expectedSize) throw new Error(`UndertaleModTool archive size mismatch: expected ${expectedSize}, received ${received}.`);
}

function assertTreeSafe(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const absolute = path.join(directory, entry.name);
        const stat = fs.lstatSync(absolute);
        if (stat.isSymbolicLink()) throw new Error(`UndertaleModTool archive contains a link: ${entry.name}`);
        if (stat.isDirectory()) assertTreeSafe(absolute);
        else if (!stat.isFile()) throw new Error(`UndertaleModTool archive contains an unsupported entry: ${entry.name}`);
    }
}

async function acquire(provenance, target) {
    const artifact = provenance.artifacts[target];
    const destination = installPath(root, provenance, target);
    try {
        verifyInstallation(root, provenance, target);
        console.log(`UndertaleModCli ${target} is already checksum-verified.`);
        return;
    } catch (error) {
        if (fs.existsSync(destination)) throw new Error(`${error.message} Remove the invalid ignored tools directory first.`);
    }
    const toolsRoot = path.join(root, 'tools', 'undertale-mod-tool');
    fs.mkdirSync(toolsRoot, { recursive: true });
    const nonce = `${process.pid}-${crypto.randomBytes(6).toString('hex')}`;
    const archive = path.join(toolsRoot, `.${target}-${nonce}.zip`);
    const staging = path.join(toolsRoot, `.${target}-${nonce}.staging`);
    try {
        await download(artifact.archiveUrl, archive, artifact.archiveSize);
        const archiveHash = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
        if (archiveHash !== artifact.archiveSha256) throw new Error(`UndertaleModTool archive checksum mismatch for ${target}.`);
        const entries = await sevenZip.list(archive);
        for (const entry of entries) {
            const name = String(entry.name || '').replaceAll('\\', '/');
            if (!name || name.startsWith('/') || /^[A-Za-z]:/.test(name) || name.split('/').includes('..') || /L/i.test(String(entry.attr || '').slice(0, 1))) {
                throw new Error(`Unsafe UndertaleModTool archive entry: ${name || '<empty>'}`);
            }
        }
        fs.mkdirSync(staging);
        await sevenZip.unpack(archive, staging);
        assertTreeSafe(staging);
        const executable = path.join(staging, artifact.executable);
        const executableHash = crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex');
        if (executableHash !== artifact.executableSha256 || !fs.existsSync(path.join(staging, 'LICENSE.txt'))) {
            throw new Error(`UndertaleModTool extracted files failed verification for ${target}.`);
        }
        const tree = treeDigest(staging);
        if (tree.fileCount !== artifact.treeFileCount || tree.sha256 !== artifact.treeSha256) {
            throw new Error(`UndertaleModTool extracted tree failed verification for ${target}.`);
        }
        fs.renameSync(staging, destination);
        if (!target.startsWith('win32-')) fs.chmodSync(path.join(destination, artifact.executable), 0o755);
        verifyInstallation(root, provenance, target);
        console.log(`UndertaleModCli ${target} acquired and checksum-verified.`);
    } finally {
        fs.rmSync(archive, { force: true });
        fs.rmSync(staging, { recursive: true, force: true });
    }
}

async function main() {
    const provenance = loadProvenance(root);
    for (const target of targets(provenance)) await acquire(provenance, target);
}

if (require.main === module) main().catch(error => { console.error(error.message); process.exitCode = 1; });

module.exports = { assertTreeSafe, download };
