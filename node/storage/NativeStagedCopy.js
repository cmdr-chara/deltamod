// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const MAX_LINE_BYTES = 1024 * 1024;
const MAX_OUTPUT_BYTES = 64 * 1024 * 1024;
const MAX_EVENTS = 100000;
const ERROR_CODES = new Set([
    'COPY_CANCELLED', 'SOURCE_NOT_DIRECTORY', 'SOURCE_UNREADABLE', 'SOURCE_LINK_BLOCKED',
    'SOURCE_ENTRY_UNSUPPORTED', 'SOURCE_CHANGED', 'SOURCE_OVERFLOW', 'DESTINATION_EXISTS',
    'DESTINATION_PARENT_CHANGED', 'STAGING_COLLISION', 'NON_ATOMIC_COMMIT',
    'INSUFFICIENT_SPACE', 'COPY_FAILED'
]);

function nativeFailure(message) {
    const error = new Error(`Native staged copy failed: ${message}`);
    error.code = 'COPY_NATIVE_FAILED';
    error.details = { operation: 'native-copy' };
    return error;
}

function exactKeys(value, expected) {
    return value && typeof value === 'object' && !Array.isArray(value)
        && Object.keys(value).sort().join(',') === [...expected].sort().join(',');
}

function safeInteger(value) {
    return Number.isSafeInteger(value) && value >= 0;
}

function validateRelative(value) {
    if (typeof value !== 'string' || !value || value.length > 32768 || value.includes('\0')
        || value.includes('\\') || path.isAbsolute(value) || path.win32.isAbsolute(value)
        || value.split('/').some(part => !part || part === '.' || part === '..')) {
        throw nativeFailure('unsafe relative path');
    }
    return value;
}

function parseEvent(line, state) {
    let event;
    try { event = JSON.parse(line); } catch { throw nativeFailure('event is not valid JSON'); }
    if (!event || typeof event !== 'object' || Array.isArray(event) || typeof event.type !== 'string') {
        throw nativeFailure('invalid event schema');
    }
    if (++state.eventCount > MAX_EVENTS) throw nativeFailure('event count exceeds limit');

    if (event.type === 'entry') {
        if (state.inventory || state.commit || state.done || !exactKeys(event, ['type', 'entryType', 'relative', 'size'])) {
            throw nativeFailure('invalid entry event');
        }
        const relative = validateRelative(event.relative);
        if (!['file', 'directory'].includes(event.entryType) || !safeInteger(event.size)
            || (event.entryType === 'directory' && event.size !== 0) || state.paths.has(relative)) {
            throw nativeFailure('invalid entry event');
        }
        state.paths.add(relative);
        state.entries.push({
            type: event.entryType,
            absolute: path.join(state.source, ...relative.split('/')),
            relative: relative.split('/').join(path.sep),
            size: event.size
        });
        if (event.entryType === 'file') {
            state.entryFiles++;
            state.entryBytes += event.size;
            if (!safeInteger(state.entryBytes)) throw nativeFailure('entry byte total exceeds limit');
        }
        return null;
    }
    if (event.type === 'inventory') {
        if (state.inventory || state.commit || state.done
            || !exactKeys(event, ['type', 'sourceRoot', 'fileCount', 'totalBytes'])
            || typeof event.sourceRoot !== 'string' || path.resolve(event.sourceRoot) !== state.source
            || !safeInteger(event.fileCount) || !safeInteger(event.totalBytes)
            || event.fileCount !== state.entryFiles || event.totalBytes !== state.entryBytes) {
            throw nativeFailure('invalid inventory event');
        }
        state.inventory = event;
        return null;
    }
    if (event.type === 'progress') {
        if (!state.inventory || state.commit || state.done
            || !exactKeys(event, ['type', 'completed', 'total', 'currentItem'])
            || !safeInteger(event.completed) || event.completed < state.completed
            || event.total !== state.inventory.totalBytes || event.completed > event.total) {
            throw nativeFailure('invalid progress event');
        }
        validateRelative(event.currentItem);
        state.completed = event.completed;
        return { phase: 'copy', completed: event.completed, total: event.total, currentItem: event.currentItem.split('/').join(path.sep) };
    }
    if (event.type === 'commit') {
        if (!state.inventory || state.commit || state.done
            || !exactKeys(event, ['type', 'completed', 'total', 'currentItem'])
            || event.completed !== state.inventory.totalBytes || event.total !== state.inventory.totalBytes
            || event.currentItem !== path.basename(state.destination)) {
            throw nativeFailure('invalid commit event');
        }
        state.commit = true;
        return { phase: 'commit', completed: event.completed, total: event.total, currentItem: event.currentItem };
    }
    if (event.type === 'done') {
        if (!state.inventory || !state.commit || state.done || !exactKeys(event, ['type'])) throw nativeFailure('invalid completion event');
        state.done = true;
        return null;
    }
    if (event.type === 'error') {
        if (state.done || !exactKeys(event, ['type', 'code', 'message']) || !ERROR_CODES.has(event.code)
            || typeof event.message !== 'string' || !event.message || event.message.length > 1024) {
            throw nativeFailure('invalid error event');
        }
        state.workerError = { code: event.code, message: event.message };
        return null;
    }
    throw nativeFailure('unknown event type');
}

function sidecarPath(override) {
    if (override) return path.resolve(override);
    if (process.platform !== 'win32' || process.arch !== 'x64') return null;
    const executable = 'deltamod-copy-worker.exe';
    if (process.resourcesPath) {
        const unpacked = path.join(process.resourcesPath, 'app.asar.unpacked', 'native', 'copy-worker', 'bin', 'win32-x64', executable);
        if (fs.existsSync(unpacked)) return unpacked;
    }
    const packaged = path.join(__dirname, '..', '..', 'native', 'copy-worker', 'bin', 'win32-x64', executable);
    if (fs.existsSync(packaged)) return packaged;
    return path.join(__dirname, '..', '..', 'native', 'target', 'debug', executable);
}

function runNativeStagedCopy(options) {
    const executable = sidecarPath(options.sidecarPath);
    if (!executable) return null;
    if (!fs.existsSync(executable)) return Promise.reject(nativeFailure('worker binary is missing'));
    const source = path.resolve(options.source);
    const destination = path.resolve(options.destination);
    const operationId = options.operationId;
    const retries = Math.max(1, options.retries ?? 3);
    if (typeof operationId !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(operationId)
        || !Number.isSafeInteger(retries) || retries > 10
        || (options.availableBytes !== null && (!safeInteger(options.availableBytes)))) {
        return Promise.reject(nativeFailure('invalid arguments'));
    }
    const staging = path.join(path.dirname(destination), `.${path.basename(destination)}.importing-${operationId}`);
    let stagingWasMissing = false;
    try { fs.lstatSync(staging); } catch (error) { if (error.code === 'ENOENT') stagingWasMissing = true; }

    return new Promise((resolve, reject) => {
        const child = spawn(executable, [source, destination, operationId, String(retries), options.availableBytes === null ? 'null' : String(options.availableBytes)], {
            windowsHide: true,
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe']
        });
        const state = {
            source, destination, entries: [], paths: new Set(), entryFiles: 0, entryBytes: 0,
            inventory: null, completed: 0, commit: false, done: false, workerError: null,
            eventCount: 0
        };
        let buffer = '';
        let outputBytes = 0;
        let stderrBytes = 0;
        let failure = null;
        let aborted = options.signal?.aborted === true;
        const abort = () => {
            // Once commit starts, cancellation can no longer guarantee that the
            // destination was not atomically published.
            if (state.commit) return;
            aborted = true;
            child.kill();
        };
        options.signal?.addEventListener('abort', abort, { once: true });
        child.stdout.setEncoding('utf8');
        child.stdout.on('data', chunk => {
            if (failure) return;
            outputBytes += Buffer.byteLength(chunk);
            if (outputBytes > MAX_OUTPUT_BYTES) {
                failure = nativeFailure('output exceeds size limit');
                child.kill();
                return;
            }
            buffer += chunk;
            const lines = buffer.split('\n');
            buffer = lines.pop();
            try {
                if (Buffer.byteLength(buffer) > MAX_LINE_BYTES) throw nativeFailure('line exceeds size limit');
                for (const line of lines) {
                    if (!line) throw nativeFailure('empty event line');
                    if (Buffer.byteLength(line) > MAX_LINE_BYTES) throw nativeFailure('line exceeds size limit');
                    const progress = parseEvent(line, state);
                    if (progress) options.onProgress?.({ operationId, ...progress });
                }
            } catch (error) {
                failure = error.code === 'COPY_NATIVE_FAILED' ? error : nativeFailure(error.message);
                child.kill();
            }
        });
        child.stderr.on('data', chunk => {
            stderrBytes += chunk.length;
            if (stderrBytes > MAX_LINE_BYTES) child.kill();
        });
        child.on('error', error => { failure ||= nativeFailure(error.message); });
        child.on('close', async code => {
            options.signal?.removeEventListener('abort', abort);
            if (aborted && stagingWasMissing) {
                try { await fs.promises.rm(staging, { recursive: true, force: true }); } catch {}
            }
            if (aborted) {
                const error = new Error('The import was cancelled.');
                error.code = 'COPY_CANCELLED';
                reject(error);
                return;
            }
            if (failure) { reject(failure); return; }
            if (buffer || stderrBytes) { reject(nativeFailure('worker produced malformed output')); return; }
            if (state.workerError) {
                const error = new Error(state.workerError.message);
                error.code = state.workerError.code;
                error.details = { source, destination, operation: state.workerError.code === 'COPY_FAILED' ? 'copyfile' : 'native-copy' };
                if (state.workerError.code === 'INSUFFICIENT_SPACE') {
                    error.details.availableBytes = options.availableBytes;
                    if (state.inventory) {
                        error.details.requiredBytes = state.inventory.totalBytes
                            + Math.min(256 * 1024 * 1024, Math.ceil(state.inventory.totalBytes * 0.05));
                    }
                }
                reject(error);
                return;
            }
            if (code !== 0 || !state.done) { reject(nativeFailure(`worker exited with code ${code}`)); return; }
            resolve({
                sourceRoot: source,
                entries: state.entries,
                fileCount: state.inventory.fileCount,
                totalBytes: state.inventory.totalBytes
            });
        });
        if (aborted) child.kill();
    });
}

module.exports = {
    runNativeStagedCopy,
    sidecarPath,
    _protocol: { parseEvent, validateRelative }
};
