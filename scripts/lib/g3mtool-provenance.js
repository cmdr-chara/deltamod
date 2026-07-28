const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const TARGETS = Object.freeze({
    'win32-x64': Object.freeze({
        platform: 'win32',
        arch: 'x64',
        executable: 'G3MTool.exe',
        installDirectory: 'win-x64'
    }),
    'linux-x64': Object.freeze({
        platform: 'linux',
        arch: 'x64',
        executable: 'G3MTool',
        installDirectory: 'linux-x64'
    })
});

const REQUIRED_FILES = Object.freeze([
    'LICENSE',
    'SECURITY.md',
    'THIRD_PARTY_NOTICES.md',
    'GameSpecificData',
    'licenses'
]);

function sha256File(filePath) {
    return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function safeHttpsUrl(value, description) {
    let parsed;
    try {
        parsed = new URL(value);
    } catch {
        throw new Error(`${description} must be a valid URL.`);
    }
    if (parsed.protocol !== 'https:' || parsed.username || parsed.password) {
        throw new Error(`${description} must be credential-free HTTPS.`);
    }
    return parsed;
}

function validateProvenance(provenance) {
    if (!provenance || provenance.schemaVersion !== 1) {
        throw new Error('G3MTool provenance must use schema version 1.');
    }
    if (provenance.verified !== true || provenance.redistributionAllowed !== true) {
        throw new Error('G3MTool provenance and redistribution permission must be verified.');
    }

    const sourceUrl = safeHttpsUrl(provenance.sourceUrl, 'G3MTool source URL');
    if (sourceUrl.hostname !== 'github.com' || sourceUrl.pathname.replace(/\/$/, '') !== '/y114git/G3MTool') {
        throw new Error('G3MTool source must be pinned to the approved upstream repository.');
    }
    if (!/^[a-f0-9]{40}$/i.test(provenance.sourceRevision || '')) {
        throw new Error('G3MTool source revision must be a full Git commit SHA.');
    }
    if (!/^\d+\.\d+\.\d+$/.test(provenance.releaseTag || '')) {
        throw new Error('G3MTool release tag must be an exact semantic version.');
    }
    if (provenance.license !== 'GPL-3.0-only') {
        throw new Error('G3MTool must retain its GPL-3.0-only license identifier.');
    }

    const artifactEntries = Object.entries(provenance.artifacts || {});
    if (
        artifactEntries.length !== Object.keys(TARGETS).length
        || artifactEntries.some(([target]) => !Object.hasOwn(TARGETS, target))
    ) {
        throw new Error('G3MTool provenance must define exactly the Windows x64 and Linux x64 artifacts.');
    }

    for (const [target, artifact] of artifactEntries) {
        const expected = TARGETS[target];
        const archiveUrl = safeHttpsUrl(artifact?.archiveUrl, `${target} archive URL`);
        const expectedPath = `/y114git/G3MTool/releases/download/${provenance.releaseTag}/`;
        if (archiveUrl.hostname !== 'github.com' || !archiveUrl.pathname.startsWith(expectedPath)) {
            throw new Error(`${target} must be acquired from the pinned upstream GitHub release.`);
        }
        if (!/^[a-f0-9]{64}$/i.test(artifact?.archiveSha256 || '')) {
            throw new Error(`${target} archive SHA-256 is invalid.`);
        }
        if (!/^[a-f0-9]{64}$/i.test(artifact?.executableSha256 || '')) {
            throw new Error(`${target} executable SHA-256 is invalid.`);
        }
        if (!Number.isSafeInteger(artifact?.archiveSize) || artifact.archiveSize <= 0 || artifact.archiveSize > 250 * 1024 * 1024) {
            throw new Error(`${target} archive size is invalid.`);
        }
        if (artifact.executable !== expected.executable || artifact.installDirectory !== expected.installDirectory) {
            throw new Error(`${target} install layout does not match the application contract.`);
        }
    }

    return provenance;
}

function loadProvenance(root) {
    const manifestPath = path.join(root, 'tools', 'G3MTool.provenance.json');
    return validateProvenance(JSON.parse(fs.readFileSync(manifestPath, 'utf8')));
}

function targetForCurrentPlatform() {
    const match = Object.entries(TARGETS).find(([, target]) => (
        target.platform === process.platform && target.arch === process.arch
    ));
    if (!match) {
        throw new Error(`G3MTool is not packaged for ${process.platform}-${process.arch}.`);
    }
    return match[0];
}

function installPath(root, provenance, targetName) {
    return path.join(root, 'tools', 'g3mtool', provenance.artifacts[targetName].installDirectory);
}

function verifyInstallation(root, provenance, targetName) {
    const artifact = provenance.artifacts[targetName];
    const directory = installPath(root, provenance, targetName);
    const executable = path.join(directory, artifact.executable);
    if (!fs.existsSync(executable)) {
        throw new Error(`Missing verified G3MTool executable for ${targetName}.`);
    }
    if (sha256File(executable).toLowerCase() !== artifact.executableSha256.toLowerCase()) {
        throw new Error(`G3MTool executable checksum mismatch for ${targetName}.`);
    }
    for (const entry of REQUIRED_FILES) {
        if (!fs.existsSync(path.join(directory, entry))) {
            throw new Error(`G3MTool ${targetName} is missing required release entry: ${entry}`);
        }
    }
    return executable;
}

module.exports = {
    REQUIRED_FILES,
    TARGETS,
    installPath,
    loadProvenance,
    sha256File,
    targetForCurrentPlatform,
    validateProvenance,
    verifyInstallation
};
