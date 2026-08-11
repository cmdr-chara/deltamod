// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { parentPort, workerData } = require('worker_threads');
const { emptyCache, hashGameFile, saveHashCache } = require('../storage/GameHashCache');
const { runNativeHashWorker } = require('./NativeHashWorker');

function listFiles(root) {
    const files = [];
    const directories = [root];
    while (directories.length) {
        const directory = directories.pop();
        for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
            const absolute = path.join(directory, entry.name);
            const stats = fs.lstatSync(absolute);
            if (stats.isSymbolicLink()) continue;
            if (stats.isDirectory()) directories.push(absolute);
            else if (stats.isFile() && stats.nlink === 1) files.push(absolute);
        }
    }
    return files;
}

function runNodeFallback() {
    const root = path.resolve(workerData.root);
    const rootStats = fs.lstatSync(root);
    if (!rootStats.isDirectory() || rootStats.isSymbolicLink()) {
        throw new Error('The game hash source is not a safe directory.');
    }

    const files = listFiles(root);
    const cache = emptyCache();
    files.forEach((file, index) => {
        const relative = path.relative(root, file);
        hashGameFile(root, relative, cache);
        parentPort.postMessage({
            operationId: workerData.operationId,
            phase: 'hashing',
            completed: index + 1,
            total: files.length,
            currentItem: relative
        });
    });
    saveHashCache(workerData.cachePath, cache);
    parentPort.postMessage({ done: true, fileCount: files.length });
}

async function main() {
    try {
        const nativeRun = runNativeHashWorker(workerData, message => parentPort.postMessage(message));
        if (nativeRun) await nativeRun;
        else runNodeFallback();
    } catch (error) {
        parentPort.postMessage({
            error: {
                code: error.code || 'HASH_CACHE_FAILED',
                message: error.message,
                stack: error.stack
            }
        });
    }
}

main();
