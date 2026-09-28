// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const fs = require('node:fs');
const path = require('node:path');

const MAX_ARTIFACT_BYTES = 512 * 1024 * 1024;
const MAX_SIGNATURE_BYTES = 16 * 1024;
const MAX_TREE_ENTRIES = 4096;
const MAX_TREE_DEPTH = 16;
const RELEASE_BASE = 'https://github.com/cmdr-chara/deltamod/releases/download';

function walk(directory) {
    const files = [];
    const pending = [{ directory, depth: 0 }];
    let count = 0;
    while (pending.length) {
        const current = pending.pop();
        const metadata = fs.lstatSync(current.directory);
        if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
            throw new Error('Updater artifact root must be an ordinary directory.');
        }
        const dir = fs.opendirSync(current.directory);
        try {
            let entry;
            while ((entry = dir.readSync()) !== null) {
                if (++count > MAX_TREE_ENTRIES) throw new Error('Updater artifact tree exceeds the entry limit.');
                const file = path.join(current.directory, entry.name);
                // Do not follow links, junctions or special files from downloaded artifacts.
                const stat = fs.lstatSync(file);
                if (stat.isSymbolicLink()) throw new Error(`Linked updater artifact is not allowed: ${entry.name}`);
                if (stat.isDirectory()) {
                    if (current.depth >= MAX_TREE_DEPTH) throw new Error('Updater artifact tree exceeds the depth limit.');
                    pending.push({ directory: file, depth: current.depth + 1 });
                } else if (stat.isFile()) {
                    files.push(file);
                } else {
                    throw new Error(`Updater artifact is not a regular file: ${entry.name}`);
                }
            }
        } finally {
            dir.closeSync();
        }
    }
    return files.sort();
}

function updaterTarget(file) {
    const name = path.basename(file).toLowerCase();
    if (!name.endsWith('.exe') && !name.endsWith('.app.tar.gz')) return null;
    const tokens = [...name.matchAll(/(?:^|[_\-. ])(x86_64|x64|amd64|aarch64|arm64|ia32|i686|x86)(?=[_\-. ]|$)/g)];
    const architectures = new Set(tokens.map(match => {
        if (['x86_64', 'x64', 'amd64'].includes(match[1])) return 'x86_64';
        if (['aarch64', 'arm64'].includes(match[1])) return 'aarch64';
        return 'x86';
    }));
    if (architectures.size !== 1) throw new Error(`Missing or ambiguous updater architecture: ${path.basename(file)}`);
    const [architecture] = architectures;
    if (name.endsWith('.exe')) {
        if (architecture !== 'x86_64') throw new Error(`Unsupported Windows updater architecture: ${architecture}`);
        return 'windows-x86_64';
    }
    if (architecture === 'x86') throw new Error('Unsupported macOS updater architecture: x86');
    return `darwin-${architecture}`;
}

function readSignature(file) {
    const before = fs.lstatSync(file);
    if (!before.isFile() || before.isSymbolicLink()) throw new Error('Updater signature must be a regular file.');
    if (before.size <= 0 || before.size > MAX_SIGNATURE_BYTES) throw new Error('Updater signature exceeds its size bound or is empty.');
    const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0) | (fs.constants.O_NONBLOCK || 0);
    const fd = fs.openSync(file, flags);
    try {
        const opened = fs.fstatSync(fd);
        if (!opened.isFile() || opened.dev !== before.dev || opened.ino !== before.ino) {
            throw new Error('Updater signature changed while opening.');
        }
        // Bound the read itself, not just the earlier stat. A growing file cannot
        // force readFileSync to allocate an unbounded buffer after the size check.
        const buffer = Buffer.alloc(MAX_SIGNATURE_BYTES + 1);
        let length = 0;
        while (length < buffer.length) {
            const read = fs.readSync(fd, buffer, length, buffer.length - length, null);
            if (!read) break;
            length += read;
        }
        if (!length || length > MAX_SIGNATURE_BYTES) throw new Error('Updater signature violates its size bound.');
        const signature = new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, length)).trim();
        if (!signature || /PRIVATE[ _-]?KEY/i.test(signature) || /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(signature)) {
            throw new Error(`Updater signature is invalid: ${path.basename(file)}`);
        }
        return signature;
    } finally {
        fs.closeSync(fd);
    }
}

function artifactMatchesVersion(name, version) {
    // Tauri's release names separate the version from the architecture with an
    // underscore or hyphen. Substrings and prerelease/build suffixes are not a
    // stable-version match (2.0.1 must not match 12.0.1, 2.0.10 or 2.0.1-beta).
    const escaped = version.replaceAll('.', '\\.');
    return new RegExp(`(?:^|[_ -])${escaped}(?:_|-(?=(?:x86_64|x64|amd64|aarch64|arm64)(?:[_ .-]|$)))`).test(name);
}

function generate(directory, tag, expectedVersion = null) {
    const match = /^community-v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(tag);
    const version = expectedVersion || (match ? match.slice(1).join('.') : null);
    if (!version || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
        throw new Error(`Invalid stable release version for tag: ${tag}`);
    }
    const rehearsalTag = new RegExp(`^updater-rehearsal-v${version.replaceAll('.', '\\.')}-run-\\d+$`);
    if (tag !== `community-v${version}` && !rehearsalTag.test(tag)) {
        throw new Error(`Release tag is not bound to version ${version}: ${tag}`);
    }
    const files = walk(directory);
    const signatures = files.filter(file => file.endsWith('.sig'));
    const platforms = {};
    for (const signatureFile of signatures) {
        const artifact = signatureFile.slice(0, -4);
        if (!fs.existsSync(artifact)) {
            throw new Error(`Signature has no matching updater artifact: ${path.basename(signatureFile)}`);
        }
        const target = updaterTarget(artifact);
        if (!target) throw new Error(`Unsupported signed updater artifact: ${path.basename(artifact)}`);
        if (platforms[target]) throw new Error(`Duplicate signed updater target: ${target}`);
        const artifactStat = fs.lstatSync(artifact);
        if (!artifactStat.isFile() || artifactStat.isSymbolicLink()) {
            throw new Error(`Updater artifact must be a regular file: ${path.basename(artifact)}`);
        }
        const size = artifactStat.size;
        if (size <= 0 || size > MAX_ARTIFACT_BYTES) {
            throw new Error(`Updater artifact violates the 512 MiB bound: ${path.basename(artifact)}`);
        }
        const signature = readSignature(signatureFile);
        const name = path.basename(artifact);
        if (!artifactMatchesVersion(name, version)) {
            throw new Error(`Updater artifact is not bound to release version ${version}: ${name}`);
        }
        platforms[target] = {
            signature,
            url: `${RELEASE_BASE}/${tag}/${encodeURIComponent(name)}`
        };
    }
    for (const target of ['windows-x86_64', 'darwin-x86_64', 'darwin-aarch64']) {
        if (!platforms[target]) throw new Error(`Missing signed updater target: ${target}`);
    }
    if (Object.keys(platforms).some(target => target.startsWith('linux-'))) {
        throw new Error('Linux .deb must not be advertised as an automatic update.');
    }
    return {
        version,
        notes: `Deltamod Community ${version}`,
        platforms
    };
}

function main() {
    const directory = process.argv[2];
    const tag = process.argv[3];
    const output = process.argv[4] || path.join(directory || '', 'latest.json');
    const version = process.argv[5] || null;
    if (!directory || !tag) {
        throw new Error('Usage: generate-tauri-updater-manifest <artifact-directory> <release-tag> [output] [version]');
    }
    const manifest = generate(path.resolve(directory), tag, version);
    fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
    console.log(`Generated signed updater metadata for ${Object.keys(manifest.platforms).length} targets.`);
}

if (require.main === module) main();

module.exports = { MAX_ARTIFACT_BYTES, MAX_SIGNATURE_BYTES, MAX_TREE_ENTRIES, MAX_TREE_DEPTH, generate, updaterTarget, artifactMatchesVersion };
