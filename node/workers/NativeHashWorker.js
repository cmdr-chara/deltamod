// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { emptyCache, saveHashCache } = require('../storage/GameHashCache');

function protocolError(message) {
    const error = new Error(`Invalid native hash worker output: ${message}`);
    error.code = 'HASH_CACHE_FAILED';
    return error;
}

function validateRelativePath(value) {
    if (typeof value !== 'string' || !value || value.includes('\0') || path.isAbsolute(value) || path.win32.isAbsolute(value)) {
        throw protocolError('unsafe relative path');
    }
    const normalized = value.replace(/\\/g, '/');
    if (normalized.split('/').some(part => !part || part === '.' || part === '..')) {
        throw protocolError('unsafe relative path');
    }
    return normalized;
}

function validateFileEvent(event, expectedTotal, seen) {
    const relative = validateRelativePath(event.relative);
    if (!/^[a-f0-9]{64}$/.test(event.sha256 || '')) throw protocolError('invalid SHA-256 digest');
    if (typeof event.signature !== 'string') throw protocolError('invalid cache signature');
    const [sizeText, mtimeText, ...extra] = event.signature.split(':');
    const size = Number(sizeText);
    const mtime = Number(mtimeText);
    if (extra.length || !/^\d+$/.test(sizeText) || !/^\d+(?:\.\d+)?(?:e[+-]?\d+)?$/i.test(mtimeText)
        || !Number.isSafeInteger(size) || size < 0 || !Number.isFinite(mtime) || mtime < 0) {
        throw protocolError('invalid cache signature');
    }
    if (!Number.isSafeInteger(event.completed) || !Number.isSafeInteger(event.total)
        || event.completed < 1 || event.total < 1 || event.completed > event.total) {
        throw protocolError('invalid progress counters');
    }
    if (event.completed !== seen.size + 1) throw protocolError('out-of-order progress');
    if (expectedTotal !== null && event.total !== expectedTotal) throw protocolError('inconsistent total');
    if (seen.has(relative)) throw protocolError('duplicate relative path');
    seen.add(relative);
    return { relative, total: event.total };
}

function sidecarPath(override) {
    if (override) return override;
    const executable = process.platform === 'win32' ? 'deltamod-hash-worker.exe' : 'deltamod-hash-worker';
    if (process.resourcesPath) {
        const unpacked = path.join(process.resourcesPath, 'app.asar.unpacked', 'native', 'hash-worker', 'bin', `${process.platform}-${process.arch}`, executable);
        if (fs.existsSync(unpacked)) return unpacked;
    }
    const packaged = path.join(__dirname, '..', '..', 'native', 'hash-worker', 'bin', `${process.platform}-${process.arch}`, executable);
    if (fs.existsSync(packaged)) return packaged;
    return path.join(__dirname, '..', '..', 'native', 'target', 'release', executable);
}

function runNativeHashWorker(workerData, onMessage) {
    const executable = sidecarPath(workerData.sidecarPath);
    if (!fs.existsSync(executable)) return null;

    return new Promise((resolve, reject) => {
        const child = spawn(executable, [path.resolve(workerData.root)], {
            windowsHide: true,
            stdio: ['ignore', 'pipe', 'pipe']
        });
        const cache = emptyCache();
        let stdout = '';
        let stderr = '';
        let doneMessage = null;
        let expectedTotal = null;
        let protocolFailure = null;
        const seen = new Set();

        child.stdout.setEncoding('utf8');
        child.stdout.on('data', chunk => {
            if (protocolFailure) return;
            stdout += chunk;
            const lines = stdout.split('\n');
            stdout = lines.pop();
            try {
                if (stdout.length > 1024 * 1024) throw protocolError('line exceeds size limit');
                for (const line of lines) {
                    if (!line.trim()) continue;
                    const event = JSON.parse(line);
                    if (event.type === 'file') {
                        if (doneMessage) throw protocolError('file event after completion');
                        const validated = validateFileEvent(event, expectedTotal, seen);
                        expectedTotal = validated.total;
                        let key = validated.relative;
                        if (process.platform === 'win32') key = key.toLowerCase();
                        cache.entries[key] = { signature: event.signature, sha256: event.sha256 };
                        onMessage({
                            operationId: workerData.operationId,
                            phase: 'hashing',
                            completed: event.completed,
                            total: event.total,
                            currentItem: validated.relative.split('/').join(path.sep)
                        });
                    } else if (event.type === 'done') {
                        if (doneMessage || !Number.isSafeInteger(event.fileCount) || event.fileCount < 0
                            || event.fileCount !== seen.size) {
                            throw protocolError('invalid completion event');
                        }
                        doneMessage = { done: true, fileCount: event.fileCount };
                    } else throw protocolError('unknown event type');
                }
            } catch (error) {
                protocolFailure = error.code === 'HASH_CACHE_FAILED' ? error : protocolError(error.message);
                child.kill();
                reject(protocolFailure);
            }
        });
        child.stderr.setEncoding('utf8');
        child.stderr.on('data', chunk => { stderr += chunk; });
        child.on('error', reject);
        child.on('close', code => {
            if (protocolFailure) return;
            if (stdout.trim()) {
                reject(protocolError('unterminated JSON line'));
                return;
            }
            if (code !== 0 || !doneMessage) {
                const error = new Error(stderr.trim() || `Native hash worker exited with code ${code}.`);
                error.code = 'HASH_CACHE_FAILED';
                reject(error);
                return;
            }
            try {
                saveHashCache(workerData.cachePath, cache);
                onMessage(doneMessage);
                resolve(doneMessage);
            } catch (error) {
                reject(error);
            }
        });
    });
}

module.exports = {
    runNativeHashWorker,
    sidecarPath,
    _protocol: { validateFileEvent, validateRelativePath }
};
