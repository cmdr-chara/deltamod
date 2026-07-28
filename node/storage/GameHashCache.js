const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { resolveWithin } = require('../security/PathSecurity');
const { readJsonSync, writeJsonAtomicSync } = require('./AtomicStore');

const SCHEMA_VERSION = 1;

function emptyCache() {
    return { schemaVersion: SCHEMA_VERSION, entries: {} };
}

function loadHashCache(cachePath) {
    const cache = readJsonSync(cachePath, null);
    if (!cache || cache.schemaVersion !== SCHEMA_VERSION || typeof cache.entries !== 'object') {
        return emptyCache();
    }
    return cache;
}

function normalizedKey(relativePath) {
    const normalized = String(relativePath).replace(/\\/g, '/');
    return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
}

function hashGameFile(gameRoot, relativePath, cache) {
    const target = resolveWithin(gameRoot, String(relativePath), { mustExist: true });
    const stats = fs.lstatSync(target);
    if (!stats.isFile() || stats.isSymbolicLink() || stats.nlink > 1) {
        const error = new Error(`Game file cannot be hashed safely: ${relativePath}`);
        error.code = 'UNSAFE_GAME_HASH_SOURCE';
        throw error;
    }

    const relative = path.relative(path.resolve(gameRoot), target);
    const key = normalizedKey(relative);
    const signature = `${stats.size}:${stats.mtimeMs}`;
    const existing = cache.entries[key];
    if (existing?.signature === signature && /^[a-f0-9]{64}$/i.test(existing.sha256 || '')) {
        return { sha256: existing.sha256, updated: false };
    }

    const sha256 = crypto.createHash('sha256').update(fs.readFileSync(target)).digest('hex');
    cache.entries[key] = { signature, sha256 };
    return { sha256, updated: true };
}

function saveHashCache(cachePath, cache) {
    cache.schemaVersion = SCHEMA_VERSION;
    writeJsonAtomicSync(cachePath, cache);
}

module.exports = {
    SCHEMA_VERSION,
    emptyCache,
    loadHashCache,
    hashGameFile,
    saveHashCache
};
