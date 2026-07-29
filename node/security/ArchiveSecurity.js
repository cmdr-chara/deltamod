// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const sevenZip = require('7zip-min');
const { validateRelativePath, isWithin } = require('./PathSecurity');

const DEFAULT_LIMITS = Object.freeze({
    maxFiles: 20_000,
    maxExpandedBytes: 4 * 1024 * 1024 * 1024,
    maxArchiveBytes: 2 * 1024 * 1024 * 1024,
    maxDepth: 32
});

class UnsafeArchiveError extends Error {
    constructor(code, message, details = {}) {
        super(message);
        this.name = 'UnsafeArchiveError';
        this.code = code;
        this.details = details;
    }
}

function normalizeLimits(limits = {}) {
    return { ...DEFAULT_LIMITS, ...limits };
}

function validateArchiveEntries(entries, limits = {}) {
    const effective = normalizeLimits(limits);
    if (!Array.isArray(entries) || entries.length === 0) {
        throw new UnsafeArchiveError('ARCHIVE_EMPTY', 'The archive is empty or could not be listed.');
    }
    if (entries.length > effective.maxFiles) {
        throw new UnsafeArchiveError(
            'ARCHIVE_FILE_LIMIT',
            `The archive contains ${entries.length} entries; the limit is ${effective.maxFiles}.`
        );
    }

    let expandedBytes = 0;
    const seen = new Set();
    for (const entry of entries) {
        const originalName = String(entry.name || '');
        let normalized;
        try {
            normalized = validateRelativePath(originalName);
        } catch (error) {
            throw new UnsafeArchiveError('ARCHIVE_UNSAFE_PATH', `Unsafe archive entry: ${originalName}`, {
                entry: originalName,
                cause: error.message
            });
        }
        if (normalized === '.' || normalized === path.sep) {
            throw new UnsafeArchiveError('ARCHIVE_UNSAFE_PATH', `Unsafe archive entry: ${originalName}`);
        }

        const depth = normalized.split(path.sep).filter(Boolean).length;
        if (depth > effective.maxDepth) {
            throw new UnsafeArchiveError(
                'ARCHIVE_NESTING_LIMIT',
                `Archive entry exceeds the nesting limit: ${originalName}`
            );
        }

        const folded = process.platform === 'win32' ? normalized.toLowerCase() : normalized;
        if (seen.has(folded)) {
            throw new UnsafeArchiveError(
                'ARCHIVE_DUPLICATE_PATH',
                `The archive contains duplicate destination paths: ${originalName}`
            );
        }
        seen.add(folded);

        const attributes = String(entry.attr || '');
        if (attributes.charAt(0) === 'l' || /(?:symbolic|hard)\s*link|reparse/i.test(attributes)) {
            throw new UnsafeArchiveError(
                'ARCHIVE_LINK_BLOCKED',
                `Archive links are not allowed: ${originalName}`
            );
        }

        const size = Number(entry.size || 0);
        if (!Number.isFinite(size) || size < 0) {
            throw new UnsafeArchiveError('ARCHIVE_INVALID_SIZE', `Invalid archive entry size: ${originalName}`);
        }
        expandedBytes += size;
        if (expandedBytes > effective.maxExpandedBytes) {
            throw new UnsafeArchiveError(
                'ARCHIVE_SIZE_LIMIT',
                `The archive expands beyond the ${effective.maxExpandedBytes}-byte limit.`
            );
        }
    }

    return { fileCount: entries.length, expandedBytes };
}

async function validateExtractedTree(root, limits = {}) {
    const effective = normalizeLimits(limits);
    const queue = [{ directory: root, depth: 0 }];
    let fileCount = 0;
    let expandedBytes = 0;
    const realRoot = await fs.promises.realpath(root);

    while (queue.length) {
        const current = queue.pop();
        const entries = await fs.promises.readdir(current.directory, { withFileTypes: true });
        for (const entry of entries) {
            const absolute = path.join(current.directory, entry.name);
            const stats = await fs.promises.lstat(absolute);
            if (stats.isSymbolicLink() || (stats.isFile() && stats.nlink > 1)) {
                throw new UnsafeArchiveError('ARCHIVE_LINK_BLOCKED', `Extracted link is not allowed: ${absolute}`);
            }

            const realPath = await fs.promises.realpath(absolute);
            if (!isWithin(realRoot, realPath, false)) {
                throw new UnsafeArchiveError('ARCHIVE_PATH_ESCAPE', `Extracted path escaped staging: ${absolute}`);
            }

            if (stats.isDirectory()) {
                if (current.depth + 1 > effective.maxDepth) {
                    throw new UnsafeArchiveError('ARCHIVE_NESTING_LIMIT', `Extracted path is nested too deeply: ${absolute}`);
                }
                queue.push({ directory: absolute, depth: current.depth + 1 });
            } else if (stats.isFile()) {
                fileCount += 1;
                expandedBytes += stats.size;
                if (fileCount > effective.maxFiles) {
                    throw new UnsafeArchiveError('ARCHIVE_FILE_LIMIT', 'Extracted file-count limit exceeded.');
                }
                if (expandedBytes > effective.maxExpandedBytes) {
                    throw new UnsafeArchiveError('ARCHIVE_SIZE_LIMIT', 'Extracted size limit exceeded.');
                }
            } else {
                throw new UnsafeArchiveError('ARCHIVE_ENTRY_TYPE', `Unsupported extracted entry: ${absolute}`);
            }
        }
    }

    return { fileCount, expandedBytes };
}

async function extractArchiveAtomic(archivePath, destination, options = {}) {
    const limits = normalizeLimits(options.limits);
    const archiveStats = await fs.promises.stat(archivePath);
    if (!archiveStats.isFile()) {
        throw new UnsafeArchiveError('ARCHIVE_NOT_FILE', 'The selected archive is not a regular file.');
    }
    if (archiveStats.size > limits.maxArchiveBytes) {
        throw new UnsafeArchiveError(
            'ARCHIVE_DOWNLOAD_LIMIT',
            `The archive exceeds the ${limits.maxArchiveBytes}-byte compressed-size limit.`
        );
    }
    if (fs.existsSync(destination)) {
        throw new UnsafeArchiveError('ARCHIVE_DESTINATION_EXISTS', `Extraction destination already exists: ${destination}`);
    }

    const listed = await sevenZip.list(archivePath);
    const inventory = validateArchiveEntries(listed, limits);
    const staging = path.join(
        path.dirname(destination),
        `.${path.basename(destination)}.extracting-${crypto.randomUUID()}`
    );

    await fs.promises.mkdir(path.dirname(destination), { recursive: true });
    await fs.promises.mkdir(staging, { recursive: true });
    try {
        await sevenZip.unpack(archivePath, staging);
        const extracted = await validateExtractedTree(staging, limits);
        await fs.promises.rename(staging, destination);
        return { destination, inventory, extracted };
    } catch (error) {
        await fs.promises.rm(staging, { recursive: true, force: true });
        throw error;
    }
}

module.exports = {
    DEFAULT_LIMITS,
    UnsafeArchiveError,
    validateArchiveEntries,
    validateExtractedTree,
    extractArchiveAtomic
};
