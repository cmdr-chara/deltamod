// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { writeJsonAtomicSync, readJsonSync } = require('./storage/AtomicStore');
const { isWithin } = require('./security/PathSecurity');
const { availableBytesFor } = require('./storage/StagedCopy');

const MIGRATION_SCHEMA_VERSION = 1;
const MANIFEST_FILE = 'community-import-manifest.json';
const JOURNAL_DIRECTORY = '.migration-transactions';
const PROFILE_ADAPTERS = Object.freeze([
    {
        id: 'official-1.7',
        migrationVersion: 1,
        supports: version => /^1\.7(?:\.|$)/.test(String(version))
    },
    {
        id: 'official-2.0',
        migrationVersion: 1,
        supports: version => /^2\.0(?:\.|$)/.test(String(version))
    }
]);

class ProfileMigrationError extends Error {
    constructor(code, message, details = {}) {
        super(message);
        this.name = 'ProfileMigrationError';
        this.code = code;
        this.details = details;
    }
}

function isAllowedTopLevel(name) {
    return /^deltamod_system-(?:\d+|unique)$/.test(name)
        || name === 'pkg.db'
        || name === 'customThemes';
}

function checkCancelled(signal) {
    if (signal?.aborted) {
        throw new ProfileMigrationError('IMPORT_CANCELLED', 'The Deltamod profile import was cancelled.');
    }
}

function listAllowedRoots(profileRoot) {
    if (!fs.existsSync(profileRoot)) return [];
    return fs.readdirSync(profileRoot, { withFileTypes: true })
        .filter(entry => isAllowedTopLevel(entry.name))
        .map(entry => ({
            name: entry.name,
            path: path.join(profileRoot, entry.name),
            directory: entry.isDirectory()
        }));
}

function readProfileStore(storePath, fallback = {}) {
    try {
        const raw = fs.readFileSync(storePath, 'utf8');
        return JSON.parse(raw.split('##')[0]);
    } catch {
        return fallback;
    }
}

function detectVersionFromStores(profileRoot) {
    const candidates = listAllowedRoots(profileRoot)
        .filter(entry => /^deltamod_system-\d+$/.test(entry.name))
        .map(entry => path.join(entry.path, 'store.json'));

    for (const storePath of candidates) {
        const store = readProfileStore(storePath, {});
        const match = String(store.version || '').match(/DELTAMOD_DATA_([0-9]+(?:\.[0-9]+){1,2})/i);
        if (match) return match[1];
    }
    return null;
}

function findInstalledVersion(localAppData) {
    if (!localAppData) return null;
    const root = path.join(localAppData, 'deltamod');
    if (!fs.existsSync(root)) return null;

    const queue = [{ directory: root, depth: 0 }];
    while (queue.length) {
        const { directory, depth } = queue.shift();
        if (depth > 4) continue;

        const packagePath = path.join(directory, 'resources', 'app', 'package.json');
        if (fs.existsSync(packagePath)) {
            const packageData = readJsonSync(packagePath, {});
            if (packageData.version) return String(packageData.version);
        }

        for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
            if (entry.isDirectory()) queue.push({ directory: path.join(directory, entry.name), depth: depth + 1 });
        }
    }

    return null;
}

function detectOfficialProfile({ appData, localAppData, destinationRoot }) {
    const sourceRoot = path.join(appData, 'deltamod');
    const exists = fs.existsSync(sourceRoot)
        && path.resolve(sourceRoot).toLowerCase() !== path.resolve(destinationRoot).toLowerCase();

    return {
        exists,
        sourceRoot,
        destinationRoot,
        version: exists
            ? findInstalledVersion(localAppData) || detectVersionFromStores(sourceRoot) || 'unknown'
            : null
    };
}

function selectProfileAdapter(version) {
    return PROFILE_ADAPTERS.find(adapter => adapter.supports(version)) || {
        id: 'official-conservative',
        migrationVersion: 1,
        supports: () => true
    };
}

async function scanTree(root, signal) {
    const files = [];
    const queue = [{ absolute: root, relative: '' }];
    let totalBytes = 0;

    while (queue.length) {
        checkCancelled(signal);
        const current = queue.pop();
        const entries = await fs.promises.readdir(current.absolute, { withFileTypes: true });

        for (const entry of entries) {
            checkCancelled(signal);
            const absolute = path.join(current.absolute, entry.name);
            const relative = path.join(current.relative, entry.name);
            const stats = await fs.promises.lstat(absolute);

            if (stats.isSymbolicLink() || (stats.isFile() && stats.nlink > 1)) {
                throw new ProfileMigrationError(
                    'UNSAFE_PROFILE_ENTRY',
                    `The source profile contains a link or reparse point: ${relative}`,
                    { relative }
                );
            }

            if (stats.isDirectory()) {
                queue.push({ absolute, relative });
            } else if (stats.isFile()) {
                totalBytes += stats.size;
                files.push({ absolute, relative, size: stats.size });
            }
        }
    }

    files.sort((a, b) => a.relative.localeCompare(b.relative));
    return { files, totalBytes };
}

async function inspectProfile(sourceRoot, options = {}) {
    const roots = listAllowedRoots(sourceRoot);
    const inventory = [];
    let totalBytes = 0;
    let fileCount = 0;

    for (const root of roots) {
        checkCancelled(options.signal);
        if (!root.directory) continue;
        const scanned = await scanTree(root.path, options.signal);
        inventory.push({
            name: root.name,
            fileCount: scanned.files.length,
            totalBytes: scanned.totalBytes
        });
        totalBytes += scanned.totalBytes;
        fileCount += scanned.files.length;
    }

    const installations = roots.filter(root => /^deltamod_system-\d+$/.test(root.name)).length;
    const modRoot = roots.find(root => root.name === 'pkg.db');
    const themeRoot = path.join(sourceRoot, 'customThemes', 'data');

    const version = options.version || detectVersionFromStores(sourceRoot) || 'unknown';
    const adapter = selectProfileAdapter(version);
    return {
        sourceRoot,
        version,
        adapter: adapter.id,
        installations,
        mods: modRoot && fs.existsSync(modRoot.path)
            ? fs.readdirSync(modRoot.path, { withFileTypes: true }).filter(entry => entry.isDirectory()).length
            : 0,
        themes: fs.existsSync(themeRoot)
            ? fs.readdirSync(themeRoot).filter(name => name.endsWith('.theme.json')).length
            : 0,
        fileCount,
        totalBytes,
        destinationRoot: options.destinationRoot || null,
        previousImport: options.destinationRoot
            ? readJsonSync(path.join(options.destinationRoot, MANIFEST_FILE), null)
            : null,
        inventory
    };
}

async function copyFileWithRetries(source, destination, options) {
    const attempts = Math.max(1, options.retries ?? 3);
    let lastError;

    for (let attempt = 1; attempt <= attempts; attempt++) {
        checkCancelled(options.signal);
        try {
            await fs.promises.mkdir(path.dirname(destination), { recursive: true });
            await fs.promises.copyFile(source, destination);
            return;
        } catch (error) {
            lastError = error;
            if (!['EIO', 'EBUSY', 'EPERM', 'EACCES'].includes(error.code) || attempt === attempts) break;
            await new Promise(resolve => setTimeout(resolve, 150 * attempt));
        }
    }

    throw new ProfileMigrationError(
        'COPY_FAILED',
        `Failed to copy ${source}: ${lastError.message}`,
        { source, destination, cause: lastError.code }
    );
}

async function hashFile(filePath, signal) {
    const hash = crypto.createHash('sha256');
    const stream = fs.createReadStream(filePath);
    for await (const chunk of stream) {
        checkCancelled(signal);
        hash.update(chunk);
    }
    return hash.digest('hex');
}

async function filesMatch(first, second) {
    if (!fs.existsSync(first) || !fs.existsSync(second)) return false;
    const [firstStats, secondStats] = await Promise.all([
        fs.promises.stat(first),
        fs.promises.stat(second)
    ]);
    if (firstStats.size !== secondStats.size) return false;
    const [firstHash, secondHash] = await Promise.all([hashFile(first), hashFile(second)]);
    return firstHash === secondHash;
}

async function hashDirectory(directory, signal) {
    const scanned = await scanTree(directory, signal);
    const hash = crypto.createHash('sha256');
    for (const file of scanned.files) {
        checkCancelled(signal);
        hash.update(file.relative.replaceAll('\\', '/'));
        hash.update('\0');
        hash.update(await hashFile(file.absolute, signal));
        hash.update('\0');
    }
    return hash.digest('hex');
}

async function buildSourceObjectManifest(sourceRoot, signal, onProgress) {
    const roots = listAllowedRoots(sourceRoot).filter(root => root.directory);
    const objects = { installations: [], mods: [], themes: [] };
    const hashes = {};
    let completed = 0;

    for (const root of roots) {
        checkCancelled(signal);
        hashes[root.name] = await hashDirectory(root.path, signal);
        completed += 1;
        onProgress?.({
            phase: 'hash',
            completed,
            total: roots.length,
            currentItem: root.name
        });

        if (/^deltamod_system-\d+$/.test(root.name)) {
            objects.installations.push({ source: root.name, hash: hashes[root.name] });
        } else if (root.name === 'pkg.db') {
            for (const entry of await fs.promises.readdir(root.path, { withFileTypes: true })) {
                if (!entry.isDirectory()) continue;
                objects.mods.push({
                    source: entry.name,
                    hash: await hashDirectory(path.join(root.path, entry.name), signal)
                });
            }
        } else if (root.name === 'customThemes') {
            const dataRoot = path.join(root.path, 'data');
            if (fs.existsSync(dataRoot)) {
                for (const fileName of await fs.promises.readdir(dataRoot)) {
                    if (!fileName.endsWith('.theme.json')) continue;
                    objects.themes.push({
                        source: fileName,
                        hash: await hashFile(path.join(dataRoot, fileName), signal)
                    });
                }
            }
        }
    }
    return { hashes, objects };
}

async function directoriesMatch(first, second) {
    if (!fs.existsSync(first) || !fs.existsSync(second)) return false;
    const [firstStats, secondStats] = await Promise.all([
        fs.promises.lstat(first),
        fs.promises.lstat(second)
    ]);
    if (!firstStats.isDirectory() || !secondStats.isDirectory()) return false;
    const [firstHash, secondHash] = await Promise.all([
        hashDirectory(first),
        hashDirectory(second)
    ]);
    return firstHash === secondHash;
}

function isPristineInstallation(installationPath) {
    if (!fs.existsSync(installationPath)) return false;
    const entries = fs.readdirSync(installationPath, { withFileTypes: true });
    if (entries.some(entry => !entry.isFile() || !['store.json', '_cname'].includes(entry.name))) {
        return false;
    }

    const store = readJsonSync(path.join(installationPath, 'store.json'), {});
    const harmlessKeys = new Set(['version', 'gamePid', 'deltaruneEdition']);
    return Object.keys(store).every(key => harmlessKeys.has(key))
        && !store.gamePath
        && !store.loadedDeltarune;
}

function nextInstallationPath(destinationRoot) {
    const installations = fs.existsSync(destinationRoot)
        ? fs.readdirSync(destinationRoot)
            .map(name => name.match(/^deltamod_system-(\d+)$/))
            .filter(Boolean)
            .map(match => ({
                index: Number(match[1]),
                path: path.join(destinationRoot, match[0])
            }))
        : [];
    const pristine = installations
        .sort((a, b) => a.index - b.index)
        .find(installation => isPristineInstallation(installation.path));
    if (pristine) return { destination: pristine.path, replacePristine: true };

    const next = installations.length
        ? Math.max(...installations.map(installation => installation.index)) + 1
        : 0;
    return {
        destination: path.join(destinationRoot, `deltamod_system-${next}`),
        replacePristine: false
    };
}

function datedName(name, label) {
    const parsed = path.parse(name);
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    return `${parsed.name}-${label}-${stamp}${parsed.ext}`;
}

async function mergeDirectory(source, destination, context, relativeBase = '') {
    await fs.promises.mkdir(destination, { recursive: true });
    const entries = await fs.promises.readdir(source, { withFileTypes: true });

    for (const entry of entries) {
        checkCancelled(context.signal);
        const sourcePath = path.join(source, entry.name);
        const relative = path.join(relativeBase, entry.name);
        let destinationPath = path.join(destination, entry.name);

        if (entry.isSymbolicLink()) {
            throw new ProfileMigrationError('UNSAFE_PROFILE_ENTRY', `Refusing linked profile entry: ${relative}`);
        }

        if (entry.isDirectory()) {
            await mergeDirectory(sourcePath, destinationPath, context, relative);
            continue;
        }

        if (entry.name === 'bananapwd') {
            if (!context.migrateCredential) {
                context.loginRequired = true;
                continue;
            }
            const migrated = await context.migrateCredential(await fs.promises.readFile(sourcePath));
            if (!migrated) {
                context.loginRequired = true;
                continue;
            }
            destinationPath = fs.existsSync(destinationPath)
                ? path.join(destination, datedName(entry.name, 'from-deltamod'))
                : destinationPath;
            await fs.promises.writeFile(destinationPath, migrated, { mode: 0o600 });
            context.createdPaths.push(destinationPath);
            continue;
        }

        if (fs.existsSync(destinationPath)) {
            if (await filesMatch(sourcePath, destinationPath)) {
                context.skipped.push(relative);
                continue;
            }
            destinationPath = path.join(destination, datedName(entry.name, 'from-deltamod'));
            context.conflicts.push({ source: relative, destination: destinationPath });
        }

        await copyFileWithRetries(sourcePath, destinationPath, context);
        context.createdPaths.push(destinationPath);
        context.completedFiles += 1;
        context.completedBytes += (await fs.promises.stat(sourcePath)).size;
        context.onProgress?.({
            operationId: context.operationId,
            phase: 'commit',
            completed: context.completedBytes,
            total: context.totalBytes,
            currentItem: relative
        });
    }
}

async function mergeModDatabase(source, destination, destinationRoot, context) {
    await fs.promises.mkdir(destination, { recursive: true });
    const entries = await fs.promises.readdir(source, { withFileTypes: true });

    for (const entry of entries) {
        checkCancelled(context.signal);
        const sourcePath = path.join(source, entry.name);
        const destinationPath = path.join(destination, entry.name);
        const relative = path.join('pkg.db', entry.name);

        if (entry.isSymbolicLink()) {
            throw new ProfileMigrationError('UNSAFE_PROFILE_ENTRY', `Refusing linked mod package: ${relative}`);
        }

        if (!entry.isDirectory()) {
            let fileDestination = destinationPath;
            if (fs.existsSync(fileDestination)) {
                if (await filesMatch(sourcePath, fileDestination)) {
                    context.skipped.push(relative);
                    continue;
                }
                fileDestination = path.join(destination, datedName(entry.name, 'from-deltamod'));
                context.conflicts.push({
                    type: 'mod-database-file',
                    source: relative,
                    destination: fileDestination,
                    resolution: 'renamed'
                });
            }
            await copyFileWithRetries(sourcePath, fileDestination, context);
            context.createdPaths.push(fileDestination);
            continue;
        }

        if (!fs.existsSync(destinationPath)) {
            await fs.promises.rename(sourcePath, destinationPath);
            context.createdPaths.push(destinationPath);
            continue;
        }

        if (await directoriesMatch(sourcePath, destinationPath)) {
            context.skipped.push(relative);
            continue;
        }

        const quarantineRoot = path.join(destinationRoot, 'quarantine', 'mod-packages');
        await fs.promises.mkdir(quarantineRoot, { recursive: true });
        const quarantinePath = path.join(quarantineRoot, datedName(entry.name, 'from-deltamod'));
        await fs.promises.rename(sourcePath, quarantinePath);
        context.createdPaths.push(quarantinePath);
        context.conflicts.push({
            type: 'mod-package',
            source: relative,
            destination: quarantinePath,
            resolution: 'quarantined'
        });
    }
}

function safeThemeAsset(sourceRoot, directory, fileName) {
    if (typeof fileName !== 'string' || !fileName || path.basename(fileName) !== fileName) {
        throw new ProfileMigrationError('UNSAFE_THEME_ASSET', `Unsafe theme asset path: ${String(fileName)}`);
    }
    const asset = path.join(sourceRoot, directory, fileName);
    return fs.existsSync(asset) ? asset : null;
}

async function copyThemeAsset(source, destination, context, relative) {
    if (!source) return;
    if (fs.existsSync(destination) && await filesMatch(source, destination)) {
        context.skipped.push(relative);
        return;
    }
    await copyFileWithRetries(source, destination, context);
    context.createdPaths.push(destination);
}

async function mergeCustomThemes(source, destination, context) {
    const sourceData = path.join(source, 'data');
    if (!fs.existsSync(sourceData)) return;
    const destinationData = path.join(destination, 'data');
    const destinationImages = path.join(destination, 'img');
    const destinationMusic = path.join(destination, 'mus');
    await Promise.all([
        fs.promises.mkdir(destinationData, { recursive: true }),
        fs.promises.mkdir(destinationImages, { recursive: true }),
        fs.promises.mkdir(destinationMusic, { recursive: true })
    ]);

    for (const fileName of await fs.promises.readdir(sourceData)) {
        checkCancelled(context.signal);
        if (!fileName.endsWith('.theme.json') || path.basename(fileName) !== fileName) continue;
        const sourceConfigPath = path.join(sourceData, fileName);
        const config = readProfileStore(sourceConfigPath, null);
        if (!config || typeof config !== 'object') {
            throw new ProfileMigrationError('INVALID_THEME', `Could not read imported theme: ${fileName}`);
        }

        const originalId = /^[a-z0-9_-]+$/i.test(String(config.id || ''))
            ? String(config.id)
            : path.basename(fileName, '.theme.json').replace(/[^a-z0-9_-]/gi, '_');
        const sourceImage = safeThemeAsset(source, 'img', config.background);
        const sourceMusic = safeThemeAsset(source, 'mus', config.mainSong);
        let destinationConfigPath = path.join(destinationData, `${originalId}.theme.json`);
        const expectedImage = sourceImage ? path.join(destinationImages, path.basename(sourceImage)) : null;
        const expectedMusic = sourceMusic ? path.join(destinationMusic, path.basename(sourceMusic)) : null;

        const bundleMatches = fs.existsSync(destinationConfigPath)
            && await filesMatch(sourceConfigPath, destinationConfigPath)
            && (!sourceImage || await filesMatch(sourceImage, expectedImage))
            && (!sourceMusic || await filesMatch(sourceMusic, expectedMusic));
        if (bundleMatches) {
            context.skipped.push(path.join('customThemes', 'data', fileName));
            continue;
        }

        const hasConflict = fs.existsSync(destinationConfigPath)
            || (expectedImage && fs.existsSync(expectedImage))
            || (expectedMusic && fs.existsSync(expectedMusic));
        let destinationImage = expectedImage;
        let destinationMusicFile = expectedMusic;
        if (hasConflict) {
            const stamp = new Date().toISOString().replace(/\D/g, '').slice(0, 14);
            const renamedId = `${originalId}_from_deltamod_${stamp}`;
            config.id = renamedId;
            config.name = `${String(config.name || originalId)} (from Deltamod)`;
            destinationConfigPath = path.join(destinationData, `${renamedId}.theme.json`);
            if (sourceImage) {
                config.background = `${renamedId}${path.extname(sourceImage).toLowerCase()}`;
                destinationImage = path.join(destinationImages, config.background);
            }
            if (sourceMusic) {
                config.mainSong = `${renamedId}${path.extname(sourceMusic).toLowerCase()}`;
                destinationMusicFile = path.join(destinationMusic, config.mainSong);
            }
            context.conflicts.push({
                type: 'theme',
                source: path.join('customThemes', 'data', fileName),
                destination: destinationConfigPath,
                resolution: 'renamed'
            });
        }

        await copyThemeAsset(
            sourceImage,
            destinationImage,
            context,
            path.join('customThemes', 'img', path.basename(sourceImage || ''))
        );
        await copyThemeAsset(
            sourceMusic,
            destinationMusicFile,
            context,
            path.join('customThemes', 'mus', path.basename(sourceMusic || ''))
        );
        writeJsonAtomicSync(destinationConfigPath, config);
        context.createdPaths.push(destinationConfigPath);
    }
}

async function rewriteImportedStore(installationPath, sourceInstallationPath) {
    const storePath = path.join(installationPath, 'store.json');
    if (!fs.existsSync(storePath)) return;
    const store = readProfileStore(storePath, {});
    if (typeof store.gamePath === 'string') {
        const normalizedSource = path.resolve(sourceInstallationPath).toLowerCase();
        const normalizedGamePath = path.resolve(store.gamePath).toLowerCase();
        if (normalizedGamePath === normalizedSource || normalizedGamePath.startsWith(`${normalizedSource}${path.sep}`)) {
            store.gamePath = path.join(installationPath, 'deltaruneInstall');
        }
    }
    store.communityImportedFrom = sourceInstallationPath;
    writeJsonAtomicSync(storePath, store);
}

async function rollbackCreatedPaths(paths) {
    for (const createdPath of [...paths].reverse()) {
        try { await fs.promises.rm(createdPath, { recursive: true, force: true }); } catch {}
    }
}

async function recoverInterruptedImports(destinationRoot) {
    const normalizedDestination = path.resolve(destinationRoot);
    const journalRoot = path.join(normalizedDestination, JOURNAL_DIRECTORY);
    if (!fs.existsSync(journalRoot)) return [];
    const recovered = [];

    for (const name of await fs.promises.readdir(journalRoot)) {
        if (!name.endsWith('.json')) continue;
        const journalPath = path.join(journalRoot, name);
        const journal = readJsonSync(journalPath, null);
        if (!journal || journal.status !== 'committing') continue;

        const createdPaths = Array.isArray(journal.createdPaths) ? journal.createdPaths : [];
        for (const createdPath of createdPaths.reverse()) {
            const resolved = path.resolve(createdPath);
            if (isWithin(normalizedDestination, resolved, false)) {
                await fs.promises.rm(resolved, { recursive: true, force: true });
            }
        }

        const stagingRoot = typeof journal.stagingRoot === 'string'
            ? path.resolve(journal.stagingRoot)
            : null;
        const expectedStagingParent = path.dirname(normalizedDestination);
        if (
            stagingRoot
            && path.dirname(stagingRoot) === expectedStagingParent
            && path.basename(stagingRoot).startsWith('.deltamod-community-import-')
            && fs.existsSync(stagingRoot)
        ) {
            for (const entry of await fs.promises.readdir(stagingRoot, { withFileTypes: true })) {
                const match = entry.name.match(/^replaced-(deltamod_system-\d+)$/);
                if (!entry.isDirectory() || !match) continue;
                const restorePath = path.join(normalizedDestination, match[1]);
                if (!fs.existsSync(restorePath)) {
                    await fs.promises.rename(path.join(stagingRoot, entry.name), restorePath);
                }
            }
            await fs.promises.rm(stagingRoot, { recursive: true, force: true });
        }

        writeJsonAtomicSync(journalPath, {
            ...journal,
            status: 'rolled-back-after-restart',
            recoveredAt: new Date().toISOString(),
            createdPaths: []
        }, { backup: false });
        recovered.push(journal.operationId || path.basename(name, '.json'));
    }
    return recovered;
}

async function importProfile(options) {
    const sourceRoot = path.resolve(options.sourceRoot);
    const destinationRoot = path.resolve(options.destinationRoot);
    if (sourceRoot.toLowerCase() === destinationRoot.toLowerCase()) {
        throw new ProfileMigrationError('SHARED_PROFILE_BLOCKED', 'Official and Community profiles cannot share writable storage.');
    }

    const operationId = options.operationId || crypto.randomUUID();
    const stagingRoot = path.join(path.dirname(destinationRoot), `.deltamod-community-import-${operationId}`);
    const stagedProfile = path.join(stagingRoot, 'profile');
    const createdPaths = [];
    const replacedPlaceholders = [];
    const journalPath = path.join(destinationRoot, JOURNAL_DIRECTORY, `${operationId}.json`);

    const summary = await inspectProfile(sourceRoot, {
        destinationRoot,
        version: options.sourceVersion,
        signal: options.signal
    });
    const availableBytes = options.availableBytes
        ?? await availableBytesFor(path.dirname(destinationRoot));
    const requiredBytes = summary.totalBytes
        + Math.min(256 * 1024 * 1024, Math.ceil(summary.totalBytes * 0.05));
    if (availableBytes !== null && availableBytes < requiredBytes) {
        throw new ProfileMigrationError(
            'INSUFFICIENT_SPACE',
            `Not enough free space for profile import. Required: ${requiredBytes} bytes; available: ${availableBytes} bytes.`,
            { requiredBytes, availableBytes, destinationRoot }
        );
    }
    const sourceManifest = await buildSourceObjectManifest(
        sourceRoot,
        options.signal,
        progress => options.onProgress?.({ operationId, ...progress })
    );
    await fs.promises.rm(stagingRoot, { recursive: true, force: true });
    await fs.promises.mkdir(stagedProfile, { recursive: true });

    const context = {
        operationId,
        signal: options.signal,
        retries: options.retries,
        totalBytes: summary.totalBytes,
        completedBytes: 0,
        completedFiles: 0,
        createdPaths,
        conflicts: [],
        skipped: [],
        loginRequired: false,
        migrateCredential: options.migrateCredential,
        onProgress: options.onProgress
    };

    try {
        const stagingContext = {
            operationId,
            signal: options.signal,
            retries: options.retries,
            totalBytes: summary.totalBytes,
            completedBytes: 0,
            completedFiles: 0,
            createdPaths: [],
            conflicts: [],
            skipped: [],
            loginRequired: false,
            migrateCredential: async encryptedCredential => encryptedCredential,
            onProgress: progress => options.onProgress?.({ ...progress, phase: 'copy' })
        };

        for (const root of listAllowedRoots(sourceRoot)) {
            checkCancelled(options.signal);
            if (!root.directory) continue;
            const destination = path.join(stagedProfile, root.name);
            await mergeDirectory(root.path, destination, stagingContext, root.name);
        }

        await fs.promises.mkdir(path.dirname(journalPath), { recursive: true });
        writeJsonAtomicSync(journalPath, {
            schemaVersion: MIGRATION_SCHEMA_VERSION,
            operationId,
            status: 'committing',
            sourceRoot,
            stagingRoot,
            createdPaths: []
        }, { backup: false });

        const stagedRoots = listAllowedRoots(stagedProfile);
        for (const root of stagedRoots.filter(item => /^deltamod_system-\d+$/.test(item.name))) {
            const installationTarget = nextInstallationPath(destinationRoot);
            const destination = installationTarget.destination;
            await fs.promises.mkdir(path.dirname(destination), { recursive: true });
            if (installationTarget.replacePristine) {
                const backup = path.join(stagingRoot, `replaced-${path.basename(destination)}`);
                await fs.promises.rename(destination, backup);
                replacedPlaceholders.push({ destination, backup });
            }
            await fs.promises.rename(root.path, destination);
            createdPaths.push(destination);
            await rewriteImportedStore(destination, path.join(sourceRoot, root.name));
            const installationObject = sourceManifest.objects.installations.find(
                installation => installation.source === root.name
            );
            if (installationObject) installationObject.destination = path.basename(destination);
            writeJsonAtomicSync(journalPath, {
                schemaVersion: MIGRATION_SCHEMA_VERSION,
                operationId,
                status: 'committing',
                sourceRoot,
                stagingRoot,
                createdPaths
            }, { backup: false });
        }

        for (const rootName of ['deltamod_system-unique']) {
            const source = path.join(stagedProfile, rootName);
            if (!fs.existsSync(source)) continue;
            await mergeDirectory(source, path.join(destinationRoot, rootName), context, rootName);
        }
        const themeSource = path.join(stagedProfile, 'customThemes');
        if (fs.existsSync(themeSource)) {
            await mergeCustomThemes(themeSource, path.join(destinationRoot, 'customThemes'), context);
        }
        const modSource = path.join(stagedProfile, 'pkg.db');
        if (fs.existsSync(modSource)) {
            await mergeModDatabase(
                modSource,
                path.join(destinationRoot, 'pkg.db'),
                destinationRoot,
                context
            );
        }

        const manifest = {
            schemaVersion: MIGRATION_SCHEMA_VERSION,
            migrationVersion: selectProfileAdapter(summary.version).migrationVersion,
            adapter: summary.adapter,
            operationId,
            sourceRoot,
            sourceVersion: summary.version,
            destinationRoot,
            importedAt: new Date().toISOString(),
            installations: summary.installations,
            mods: summary.mods,
            themes: summary.themes,
            fileCount: summary.fileCount,
            totalBytes: summary.totalBytes,
            hashes: sourceManifest.hashes,
            importedObjects: sourceManifest.objects,
            conflicts: context.conflicts,
            skipped: context.skipped,
            loginRequired: context.loginRequired
        };

        writeJsonAtomicSync(path.join(destinationRoot, MANIFEST_FILE), manifest);
        writeJsonAtomicSync(journalPath, { ...manifest, status: 'completed', createdPaths }, { backup: false });
        await fs.promises.rm(stagingRoot, { recursive: true, force: true });
        return manifest;
    } catch (error) {
        await rollbackCreatedPaths(createdPaths);
        for (const replacement of replacedPlaceholders.reverse()) {
            if (fs.existsSync(replacement.backup) && !fs.existsSync(replacement.destination)) {
                await fs.promises.rename(replacement.backup, replacement.destination);
            }
        }
        await fs.promises.rm(stagingRoot, { recursive: true, force: true });
        try {
            writeJsonAtomicSync(journalPath, {
                schemaVersion: MIGRATION_SCHEMA_VERSION,
                operationId,
                status: 'rolled-back',
                sourceRoot,
                error: { code: error.code || 'IMPORT_FAILED', message: error.message },
                createdPaths: []
            }, { backup: false });
        } catch {}
        throw error;
    }
}

module.exports = {
    MIGRATION_SCHEMA_VERSION,
    MANIFEST_FILE,
    ProfileMigrationError,
    detectOfficialProfile,
    inspectProfile,
    importProfile,
    recoverInterruptedImports,
    isAllowedTopLevel,
    selectProfileAdapter
};
