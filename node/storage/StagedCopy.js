// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

class StagedCopyError extends Error {
    constructor(code, message, details = {}) {
        super(message);
        this.name = 'StagedCopyError';
        this.code = code;
        this.details = details;
    }
}

function checkCancelled(signal) {
    if (signal?.aborted) {
        throw new StagedCopyError('COPY_CANCELLED', 'The import was cancelled.');
    }
}

async function inspectSourceTree(sourceRoot, signal) {
    const source = path.resolve(sourceRoot);
    const stats = await fs.promises.lstat(source);
    if (!stats.isDirectory()) {
        throw new StagedCopyError('SOURCE_NOT_DIRECTORY', 'The selected source is not a directory.', { source });
    }

    const files = [];
    const queue = [{ absolute: source, relative: '' }];
    let totalBytes = 0;
    while (queue.length) {
        checkCancelled(signal);
        const current = queue.pop();
        let entries;
        try {
            entries = await fs.promises.readdir(current.absolute, { withFileTypes: true });
        } catch (error) {
            throw new StagedCopyError(
                'SOURCE_UNREADABLE',
                `Cannot read the source directory: ${current.absolute}`,
                { source: current.absolute, cause: error.code }
            );
        }

        for (const entry of entries) {
            checkCancelled(signal);
            const absolute = path.join(current.absolute, entry.name);
            const relative = path.join(current.relative, entry.name);
            const entryStats = await fs.promises.lstat(absolute);
            if (entryStats.isSymbolicLink() || (entryStats.isFile() && entryStats.nlink > 1)) {
                throw new StagedCopyError(
                    'SOURCE_LINK_BLOCKED',
                    `Linked or reparse-point source entries are not imported: ${relative}`,
                    { source: absolute }
                );
            }
            if (entryStats.isDirectory()) {
                files.push({ type: 'directory', absolute, relative, size: 0 });
                queue.push({ absolute, relative });
            } else if (entryStats.isFile()) {
                files.push({ type: 'file', absolute, relative, size: entryStats.size });
                totalBytes += entryStats.size;
            } else {
                throw new StagedCopyError(
                    'SOURCE_ENTRY_UNSUPPORTED',
                    `Unsupported source entry: ${relative}`,
                    { source: absolute }
                );
            }
        }
    }
    files.sort((a, b) => a.relative.localeCompare(b.relative));
    return {
        sourceRoot: source,
        entries: files,
        fileCount: files.filter(entry => entry.type === 'file').length,
        totalBytes
    };
}

async function availableBytesFor(target) {
    let ancestor = path.resolve(target);
    while (!fs.existsSync(ancestor)) {
        const parent = path.dirname(ancestor);
        if (parent === ancestor) return null;
        ancestor = parent;
    }
    if (typeof fs.promises.statfs !== 'function') return null;
    const stats = await fs.promises.statfs(ancestor);
    return Number(stats.bavail) * Number(stats.bsize);
}

async function copyFileWithProgress(source, destination, context) {
    const attempts = Math.max(1, context.retries ?? 3);
    let lastError;

    for (let attempt = 1; attempt <= attempts; attempt++) {
        checkCancelled(context.signal);
        await fs.promises.mkdir(path.dirname(destination), { recursive: true });
        await fs.promises.rm(destination, { force: true });
        let currentFileBytes = 0;

        try {
            await new Promise((resolve, reject) => {
                const input = fs.createReadStream(source);
                const output = fs.createWriteStream(destination, { flags: 'wx' });
                const abort = () => {
                    const error = new StagedCopyError('COPY_CANCELLED', 'The import was cancelled.');
                    input.destroy(error);
                    output.destroy(error);
                };
                context.signal?.addEventListener('abort', abort, { once: true });
                input.on('data', chunk => {
                    currentFileBytes += chunk.length;
                    context.onProgress?.({
                        operationId: context.operationId,
                        phase: 'copy',
                        completed: context.completedBytes + currentFileBytes,
                        total: context.totalBytes,
                        currentItem: context.currentItem
                    });
                });
                input.on('error', reject);
                output.on('error', reject);
                output.on('finish', resolve);
                output.on('close', () => context.signal?.removeEventListener('abort', abort));
                input.pipe(output);
            });
            return;
        } catch (error) {
            lastError = error;
            await fs.promises.rm(destination, { force: true });
            if (error.code === 'COPY_CANCELLED') throw error;
            if (!['EIO', 'EBUSY', 'EPERM', 'EACCES'].includes(error.code) || attempt === attempts) break;
            await new Promise(resolve => setTimeout(resolve, 150 * attempt));
        }
    }

    throw new StagedCopyError(
        'COPY_FAILED',
        `Failed to copy ${context.currentItem}: ${lastError.message}`,
        { source, destination, operation: 'copyfile', cause: lastError.code }
    );
}

async function copyDirectoryAtomic(options) {
    const destination = path.resolve(options.destination);
    if (fs.existsSync(destination)) {
        throw new StagedCopyError(
            'DESTINATION_EXISTS',
            `The destination already exists: ${destination}`,
            { destination }
        );
    }

    const operationId = options.operationId || crypto.randomUUID();
    const inventory = await inspectSourceTree(options.source, options.signal);
    const availableBytes = options.availableBytes ?? await availableBytesFor(path.dirname(destination));
    const requiredBytes = inventory.totalBytes + Math.min(256 * 1024 * 1024, Math.ceil(inventory.totalBytes * 0.05));
    if (availableBytes !== null && availableBytes < requiredBytes) {
        throw new StagedCopyError(
            'INSUFFICIENT_SPACE',
            `Not enough free space. Required: ${requiredBytes} bytes; available: ${availableBytes} bytes.`,
            { requiredBytes, availableBytes, destination }
        );
    }

    const staging = path.join(
        path.dirname(destination),
        `.${path.basename(destination)}.importing-${operationId}`
    );
    await fs.promises.rm(staging, { recursive: true, force: true });
    await fs.promises.mkdir(staging, { recursive: true });
    let completedBytes = 0;

    try {
        for (const entry of inventory.entries) {
            checkCancelled(options.signal);
            const stagedPath = path.join(staging, entry.relative);
            if (entry.type === 'directory') {
                await fs.promises.mkdir(stagedPath, { recursive: true });
                continue;
            }
            await copyFileWithProgress(entry.absolute, stagedPath, {
                operationId,
                signal: options.signal,
                retries: options.retries,
                completedBytes,
                totalBytes: inventory.totalBytes,
                currentItem: entry.relative,
                onProgress: options.onProgress
            });
            completedBytes += entry.size;
        }
        checkCancelled(options.signal);
        options.onProgress?.({
            operationId,
            phase: 'commit',
            completed: inventory.totalBytes,
            total: inventory.totalBytes,
            currentItem: path.basename(destination)
        });
        await fs.promises.rename(staging, destination);
        return { operationId, destination, ...inventory, availableBytes, requiredBytes };
    } catch (error) {
        await fs.promises.rm(staging, { recursive: true, force: true });
        throw error;
    }
}

module.exports = {
    StagedCopyError,
    inspectSourceTree,
    availableBytesFor,
    copyDirectoryAtomic
};
