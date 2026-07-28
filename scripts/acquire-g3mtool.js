const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { Transform } = require('stream');
const { pipeline } = require('stream/promises');
const sevenZip = require('7zip-min');
const {
    REQUIRED_FILES,
    installPath,
    loadProvenance,
    targetForCurrentPlatform,
    verifyInstallation
} = require('./lib/g3mtool-provenance');

const root = path.resolve(__dirname, '..');
const allowedDownloadHosts = new Set([
    'github.com',
    'objects.githubusercontent.com',
    'release-assets.githubusercontent.com'
]);

function requestedTargets(provenance) {
    const explicit = process.argv.slice(2).filter(argument => !argument.startsWith('-'));
    if (process.argv.includes('--all')) return Object.keys(provenance.artifacts);
    if (explicit.length > 1) throw new Error('Acquire at most one G3MTool target at a time.');
    const targets = explicit.length === 1 ? explicit : [targetForCurrentPlatform()];
    for (const target of targets) {
        if (!Object.hasOwn(provenance.artifacts, target)) throw new Error(`Unknown G3MTool target: ${target}`);
    }
    return targets;
}

function assertDownloadUrl(value) {
    const url = new URL(value);
    if (
        url.protocol !== 'https:'
        || url.username
        || url.password
        || !allowedDownloadHosts.has(url.hostname)
    ) {
        throw new Error(`G3MTool download redirected to an unapproved URL: ${url.toString()}`);
    }
    return url;
}

async function download(url, destination, expectedSize, redirects = 0) {
    if (redirects > 5) throw new Error('Too many redirects while acquiring G3MTool.');
    const parsed = assertDownloadUrl(url);
    const response = await new Promise((resolve, reject) => {
        const request = require('https').get(parsed, {
            headers: { 'User-Agent': 'Deltamod-Community-CI/1' },
            timeout: 30_000
        }, resolve);
        request.on('timeout', () => request.destroy(new Error('G3MTool acquisition timed out.')));
        request.on('error', reject);
    });

    if ([301, 302, 303, 307, 308].includes(response.statusCode)) {
        response.resume();
        if (!response.headers.location) throw new Error('G3MTool acquisition returned an empty redirect.');
        return download(new URL(response.headers.location, parsed).toString(), destination, expectedSize, redirects + 1);
    }
    if (response.statusCode !== 200) {
        response.resume();
        throw new Error(`G3MTool acquisition returned HTTP ${response.statusCode}.`);
    }

    const advertisedSize = Number(response.headers['content-length']);
    if (Number.isFinite(advertisedSize) && advertisedSize !== expectedSize) {
        response.resume();
        throw new Error(`G3MTool archive size mismatch: expected ${expectedSize}, received ${advertisedSize}.`);
    }

    let received = 0;
    const limiter = new Transform({
        transform(chunk, encoding, callback) {
            received += chunk.length;
            if (received > expectedSize) {
                callback(new Error('G3MTool archive exceeded its pinned size.'));
                return;
            }
            callback(null, chunk);
        }
    });
    await pipeline(response, limiter, fs.createWriteStream(destination, { flags: 'wx', mode: 0o600 }));
    if (received !== expectedSize) {
        throw new Error(`G3MTool archive was truncated: expected ${expectedSize}, received ${received}.`);
    }
}

function validateArchiveEntries(entries, artifact) {
    const allowedTopLevel = new Set([
        artifact.executable,
        ...REQUIRED_FILES
    ]);
    for (const entry of entries) {
        const name = String(entry.name || '').replaceAll('\\', '/');
        if (
            !name
            || name.startsWith('/')
            || /^[A-Za-z]:/.test(name)
            || name.split('/').includes('..')
            || !allowedTopLevel.has(name.split('/')[0])
        ) {
            throw new Error(`Unsafe or unexpected G3MTool archive entry: ${name || '<empty>'}`);
        }
        if (/L/i.test(String(entry.attr || '').slice(0, 1))) {
            throw new Error(`G3MTool archive contains a link entry: ${name}`);
        }
    }
}

function assertExtractedTreeSafe(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const absolute = path.join(directory, entry.name);
        const stat = fs.lstatSync(absolute);
        if (stat.isSymbolicLink()) throw new Error(`G3MTool extraction produced a symbolic link: ${entry.name}`);
        if (stat.isDirectory()) assertExtractedTreeSafe(absolute);
        else if (!stat.isFile()) throw new Error(`G3MTool extraction produced an unsupported entry: ${entry.name}`);
    }
}

async function acquireTarget(provenance, targetName) {
    const artifact = provenance.artifacts[targetName];
    const destination = installPath(root, provenance, targetName);
    try {
        verifyInstallation(root, provenance, targetName);
        console.log(`G3MTool ${targetName} is already present and checksum-verified.`);
        return;
    } catch (error) {
        if (fs.existsSync(destination)) {
            throw new Error(`${error.message} Remove the invalid ignored tools directory before acquiring it again.`);
        }
    }

    const toolsRoot = path.join(root, 'tools', 'g3mtool');
    fs.mkdirSync(toolsRoot, { recursive: true });
    const nonce = `${process.pid}-${crypto.randomBytes(6).toString('hex')}`;
    const archive = path.join(toolsRoot, `.${targetName}-${nonce}.zip`);
    const staging = path.join(toolsRoot, `.${targetName}-${nonce}.staging`);
    try {
        await download(artifact.archiveUrl, archive, artifact.archiveSize);
        const archiveHash = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
        if (archiveHash.toLowerCase() !== artifact.archiveSha256.toLowerCase()) {
            throw new Error(`G3MTool archive checksum mismatch for ${targetName}.`);
        }

        const entries = await sevenZip.list(archive);
        validateArchiveEntries(entries, artifact);
        fs.mkdirSync(staging, { recursive: false });
        await sevenZip.unpack(archive, staging);
        assertExtractedTreeSafe(staging);

        const stagedExecutable = path.join(staging, artifact.executable);
        if (!fs.existsSync(stagedExecutable)) throw new Error(`G3MTool archive is missing ${artifact.executable}.`);
        const executableHash = crypto.createHash('sha256').update(fs.readFileSync(stagedExecutable)).digest('hex');
        if (executableHash.toLowerCase() !== artifact.executableSha256.toLowerCase()) {
            throw new Error(`G3MTool executable checksum mismatch for ${targetName}.`);
        }
        for (const entry of REQUIRED_FILES) {
            if (!fs.existsSync(path.join(staging, entry))) {
                throw new Error(`G3MTool archive is missing required release entry: ${entry}`);
            }
        }

        fs.renameSync(staging, destination);
        if (targetName.startsWith('linux-')) fs.chmodSync(path.join(destination, artifact.executable), 0o755);
        verifyInstallation(root, provenance, targetName);
        console.log(`G3MTool ${targetName} acquired from the pinned upstream release and checksum-verified.`);
    } finally {
        fs.rmSync(archive, { force: true });
        fs.rmSync(staging, { recursive: true, force: true });
    }
}

async function main() {
    const provenance = loadProvenance(root);
    for (const targetName of requestedTargets(provenance)) {
        await acquireTarget(provenance, targetName);
    }
}

if (require.main === module) {
    main().catch(error => {
        console.error(error.message);
        process.exitCode = 1;
    });
}

module.exports = {
    assertDownloadUrl,
    assertExtractedTreeSafe,
    download,
    validateArchiveEntries
};
