// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const fs = require('node:fs');
const path = require('node:path');

const MAX_ARTIFACT_BYTES = 512 * 1024 * 1024;
const RELEASE_BASE = 'https://github.com/cmdr-chara/deltamod/releases/download';

function walk(directory) {
    return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
        const file = path.join(directory, entry.name);
        return entry.isDirectory() ? walk(file) : [file];
    });
}

function updaterTarget(file) {
    const name = path.basename(file).toLowerCase();
    if (name.endsWith('.exe')) return 'windows-x86_64';
    if (!name.endsWith('.app.tar.gz')) return null;
    if (/(?:aarch64|arm64)/.test(name)) return 'darwin-aarch64';
    if (/(?:x86_64|x64)/.test(name)) return 'darwin-x86_64';
    throw new Error(`Cannot determine macOS updater architecture: ${path.basename(file)}`);
}

function generate(directory, tag) {
    const match = /^community-v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(tag);
    if (!match) throw new Error(`Invalid stable release tag: ${tag}`);
    const version = match.slice(1).join('.');
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
        const size = fs.statSync(artifact).size;
        if (size <= 0 || size > MAX_ARTIFACT_BYTES) {
            throw new Error(`Updater artifact violates the 512 MiB bound: ${path.basename(artifact)}`);
        }
        const signature = fs.readFileSync(signatureFile, 'utf8').trim();
        if (!signature || signature.length > 16 * 1024 || /PRIVATE[ _-]?KEY/i.test(signature)) {
            throw new Error(`Updater signature is invalid: ${path.basename(signatureFile)}`);
        }
        const name = path.basename(artifact);
        if (!name.includes(version)) {
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
    if (!directory || !tag) {
        throw new Error('Usage: generate-tauri-updater-manifest <artifact-directory> <community-vX.Y.Z> [output]');
    }
    const manifest = generate(path.resolve(directory), tag);
    fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
    console.log(`Generated signed updater metadata for ${Object.keys(manifest.platforms).length} targets.`);
}

if (require.main === module) main();

module.exports = { MAX_ARTIFACT_BYTES, generate, updaterTarget };
