// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function inventory(root, { maxFiles = 200000, maxBytes = 4 * 1024 ** 4 } = {}) {
    root = path.resolve(root);
    const files = [];
    const queue = [root];
    let bytes = 0;
    while (queue.length) {
        const current = queue.pop();
        const stat = fs.lstatSync(current);
        if (stat.isSymbolicLink()) throw new Error(`Linked package entry: ${current}`);
        if (stat.isDirectory()) {
            for (const entry of fs.readdirSync(current).sort()) queue.push(path.join(current, entry));
        } else {
            if (!stat.isFile()) throw new Error(`Non-regular package entry: ${current}`);
            bytes += stat.size;
            if (files.length >= maxFiles || !Number.isSafeInteger(bytes) || bytes > maxBytes) throw new Error('Package inventory exceeded bounds');
            files.push({ path: path.relative(root, current).split(path.sep).join('/'), bytes: stat.size });
        }
    }
    files.sort((a, b) => b.bytes - a.bytes || a.path.localeCompare(b.path));
    return { bytes, fileCount: files.length, files };
}

function category(file) {
    const normalized = `/${file.toLowerCase()}`;
    for (const name of ['g3mtool', 'undertale-mod-tool', 'butler']) {
        if (normalized.includes(`/${name}/`)) return name;
    }
    if (normalized.includes('/themes/')) return 'themes';
    if (/deltamod-.*-worker(?:\.exe)?$/.test(normalized)) return 'native-workers';
    if (/deltamod-tauri-shell(?:\.exe)?$/.test(normalized)) return 'shell';
    return 'other';
}

async function digest(file) {
    const hash = crypto.createHash('sha256');
    for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
    return hash.digest('hex');
}

async function audit(root, installer) {
    if (!fs.lstatSync(root).isDirectory()) throw new Error('Installed footprint requires a directory');
    const data = inventory(root);
    const categories = {};
    const sameSize = new Map();
    for (const file of data.files) {
        const name = category(file.path);
        categories[name] = (categories[name] || 0) + file.bytes;
        if (file.bytes >= 4096) {
            const peers = sameSize.get(file.bytes) || [];
            peers.push(file);
            sameSize.set(file.bytes, peers);
        }
    }
    const duplicates = [];
    for (const group of sameSize.values()) {
        if (group.length < 2) continue;
        const hashes = new Map();
        for (const file of group) {
            const sha256 = await digest(path.join(root, file.path));
            const paths = hashes.get(sha256) || [];
            paths.push(file.path);
            hashes.set(sha256, paths);
        }
        for (const [sha256, paths] of hashes) {
            if (paths.length > 1) duplicates.push({ sha256, bytesEach: group[0].bytes, redundantBytes: group[0].bytes * (paths.length - 1), paths });
        }
    }
    duplicates.sort((a, b) => b.redundantBytes - a.redundantBytes);
    let download = null;
    if (installer) {
        const stat = fs.lstatSync(installer);
        if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Installer must be a regular file');
        download = { name: path.basename(installer), bytes: stat.size, sha256: await digest(installer) };
    }
    return {
        schemaVersion: 1,
        installed: { bytes: data.bytes, fileCount: data.fileCount, categories, largestFiles: data.files.slice(0, 30) },
        installer: download,
        duplicates,
        note: 'Logical installed file bytes, not filesystem allocation. Installer download is a separate measure.'
    };
}

if (require.main === module) {
    const [root, installer, output] = process.argv.slice(2);
    if (!root || !output) throw new Error('Usage: node package-footprint.cjs <installed-directory> <installer> <output.json>');
    audit(root, installer).then(result => fs.writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx' }))
        .catch(error => { console.error(error.message); process.exitCode = 1; });
}
module.exports = { inventory, category, audit };
