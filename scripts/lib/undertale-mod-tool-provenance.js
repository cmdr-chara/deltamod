// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const TARGETS = Object.freeze({
    'win32-x64': { platform: 'win32', arch: 'x64', executable: 'UndertaleModCli.exe', installDirectory: 'win-x64' },
    'linux-x64': { platform: 'linux', arch: 'x64', executable: 'UndertaleModCli', installDirectory: 'linux-x64' },
    'darwin-x64': { platform: 'darwin', arch: 'x64', executable: 'UndertaleModCli', installDirectory: 'mac-x64' }
});

function sha256File(filePath) {
    return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function treeDigest(directory, options = {}) {
    const entries = [];
    const visit = (current, relative = '') => {
        for (const entry of fs.readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
            const absolute = path.join(current, entry.name);
            const childRelative = path.posix.join(relative, entry.name);
            if (options.excludeDebugSymbols && childRelative === 'UndertaleModCli.pdb') continue;
            const stat = fs.lstatSync(absolute);
            if (stat.isSymbolicLink()) throw new Error(`UndertaleModCli installation contains a link: ${childRelative}`);
            if (stat.isDirectory()) visit(absolute, childRelative);
            else if (stat.isFile()) entries.push(`${childRelative}\0${stat.size}\0${sha256File(absolute)}\n`);
            else throw new Error(`UndertaleModCli installation contains an unsupported entry: ${childRelative}`);
        }
    };
    visit(directory);
    return {
        fileCount: entries.length,
        sha256: crypto.createHash('sha256').update(entries.join('')).digest('hex')
    };
}

function validateProvenance(value) {
    if (!value || value.schemaVersion !== 1 || value.verified !== true || value.redistributionAllowed !== true) {
        throw new Error('UndertaleModTool provenance must be verified and use schema version 1.');
    }
    const source = new URL(value.sourceUrl);
    if (source.protocol !== 'https:' || source.hostname !== 'github.com' || source.pathname.replace(/\/$/, '') !== '/UnderminersTeam/UndertaleModTool') {
        throw new Error('UndertaleModTool source repository is invalid.');
    }
    if (
        !/^[a-f0-9]{40}$/i.test(value.sourceRevision || '')
        || value.releaseRevision !== value.sourceRevision
        || value.license !== 'GPL-3.0-only'
    ) {
        throw new Error('UndertaleModTool source revision or license is invalid.');
    }
    const entries = Object.entries(value.artifacts || {});
    if (entries.length !== Object.keys(TARGETS).length || entries.some(([name]) => !TARGETS[name])) {
        throw new Error('UndertaleModTool provenance has an invalid target set.');
    }
    for (const [name, artifact] of entries) {
        const expected = TARGETS[name];
        const archive = new URL(artifact.archiveUrl);
        if (archive.protocol !== 'https:' || archive.hostname !== 'github.com' || !archive.pathname.startsWith(`/UnderminersTeam/UndertaleModTool/releases/download/${value.releaseTag}/`)) {
            throw new Error(`UndertaleModTool ${name} archive URL is invalid.`);
        }
        if (!Number.isSafeInteger(artifact.archiveSize) || artifact.archiveSize <= 0 || artifact.archiveSize > 250 * 1024 * 1024) {
            throw new Error(`UndertaleModTool ${name} archive size is invalid.`);
        }
        if (
            !/^[a-f0-9]{64}$/i.test(artifact.archiveSha256 || '')
            || !/^[a-f0-9]{64}$/i.test(artifact.executableSha256 || '')
            || !/^[a-f0-9]{64}$/i.test(artifact.treeSha256 || '')
            || !/^[a-f0-9]{64}$/i.test(artifact.packageTreeSha256 || '')
            || !Number.isSafeInteger(artifact.treeFileCount)
            || artifact.treeFileCount <= 0
            || !Number.isSafeInteger(artifact.packageTreeFileCount)
            || artifact.packageTreeFileCount <= 0
        ) {
            throw new Error(`UndertaleModTool ${name} checksums are invalid.`);
        }
        if (artifact.executable !== expected.executable || artifact.installDirectory !== expected.installDirectory) {
            throw new Error(`UndertaleModTool ${name} layout is invalid.`);
        }
    }
    return value;
}

function loadProvenance(root) {
    return validateProvenance(JSON.parse(fs.readFileSync(path.join(root, 'tools', 'UndertaleModTool.provenance.json'), 'utf8')));
}

function targetForCurrentPlatform() {
    const match = Object.entries(TARGETS).find(([, value]) => value.platform === process.platform && value.arch === process.arch);
    if (!match) throw new Error(`UndertaleModCli is not packaged for ${process.platform}-${process.arch}.`);
    return match[0];
}

function installPath(root, provenance, target) {
    return path.join(root, 'tools', 'undertale-mod-tool', provenance.artifacts[target].installDirectory);
}

function verifyInstallation(root, provenance, target) {
    const artifact = provenance.artifacts[target];
    const directory = installPath(root, provenance, target);
    const executable = path.join(directory, artifact.executable);
    if (!fs.existsSync(executable) || sha256File(executable) !== artifact.executableSha256) {
        throw new Error(`UndertaleModCli executable verification failed for ${target}.`);
    }
    if (!fs.existsSync(path.join(directory, 'LICENSE.txt'))) {
        throw new Error(`UndertaleModCli ${target} is missing LICENSE.txt.`);
    }
    const tree = treeDigest(directory);
    if (tree.fileCount !== artifact.treeFileCount || tree.sha256 !== artifact.treeSha256) {
        throw new Error(`UndertaleModCli installation tree verification failed for ${target}.`);
    }
    return executable;
}

function verifyPackagedInstallation(root, provenance, target) {
    const artifact = provenance.artifacts[target];
    const directory = installPath(root, provenance, target);
    const executable = path.join(directory, artifact.executable);
    if (!fs.existsSync(executable) || sha256File(executable) !== artifact.executableSha256) {
        throw new Error(`Packaged UndertaleModCli executable verification failed for ${target}.`);
    }
    if (!fs.existsSync(path.join(directory, 'LICENSE.txt'))) {
        throw new Error(`Packaged UndertaleModCli ${target} is missing LICENSE.txt.`);
    }
    const tree = treeDigest(directory, { excludeDebugSymbols: true });
    if (tree.fileCount !== artifact.packageTreeFileCount || tree.sha256 !== artifact.packageTreeSha256) {
        throw new Error(`Packaged UndertaleModCli installation tree verification failed for ${target}.`);
    }
    return executable;
}

module.exports = { TARGETS, installPath, loadProvenance, sha256File, targetForCurrentPlatform, treeDigest, validateProvenance, verifyInstallation, verifyPackagedInstallation };
