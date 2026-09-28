// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const { TextDecoder } = require('node:util');

const MAX_INPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_BYTES = 8 * 1024;
const MAX_TIMEOUT_MS = 30_000;
const VALIDATION_CODES = new Set(['PATCH_PLAN_INVALID', 'PATCH_PLAN_IO']);

function nativeFailure(message) {
    const error = new Error(`Native patch-plan validation failed: ${message}`);
    error.code = 'PATCH_PLAN_NATIVE_FAILED';
    return error;
}

function exactKeys(value, expected) {
    return value && typeof value === 'object' && !Array.isArray(value)
        && Object.keys(value).sort().join(',') === [...expected].sort().join(',');
}

function safeCount(value) {
    return Number.isSafeInteger(value) && value >= 0;
}

function sidecarPath(override) {
    if (override) return path.resolve(override);
    if (process.platform !== 'win32' || process.arch !== 'x64') return null;

    const executable = 'deltamod-patch-plan-worker.exe';
    const packaged = path.join(__dirname, '..', '..', 'native', 'patch-plan-worker', 'bin', 'win32-x64', executable);
    if (fs.existsSync(packaged)) return packaged;
    return path.join(__dirname, '..', '..', 'native', 'target', 'debug', executable);
}

function parseResponse(output) {
    if (Buffer.byteLength(output, 'utf8') > MAX_OUTPUT_BYTES) throw nativeFailure('response exceeds size limit');
    if (!output.endsWith('\n') || output.slice(0, -1).includes('\n')) throw nativeFailure('expected one JSON response');
    let response;
    try { response = JSON.parse(output); } catch { throw nativeFailure('response is not valid JSON'); }

    if (response?.ok === true) {
        if (!exactKeys(response, ['ok', 'operationCount', 'patchCount', 'snapshotCount'])
            || !safeCount(response.operationCount) || !safeCount(response.patchCount)
            || !safeCount(response.snapshotCount)) {
            throw nativeFailure('invalid success response');
        }
        return {
            operationCount: response.operationCount,
            patchCount: response.patchCount,
            snapshotCount: response.snapshotCount
        };
    }
    if (response?.ok === false) {
        if (!exactKeys(response, ['ok', 'code', 'message']) || !VALIDATION_CODES.has(response.code)
            || typeof response.message !== 'string' || !response.message || response.message.length > 512) {
            throw nativeFailure('invalid failure response');
        }
        const error = new Error(response.message);
        error.code = response.code;
        throw error;
    }
    throw nativeFailure('invalid response schema');
}

function runWorker(executable, input, timeoutMs, signal, start = spawn) {
    return new Promise((resolve, reject) => {
        if (signal?.aborted) { reject(nativeFailure('request was cancelled')); return; }
        let child;
        try {
            child = start(executable, [], {
                windowsHide: true, shell: false, stdio: ['pipe', 'pipe', 'pipe']
            });
        } catch { reject(nativeFailure('worker could not be started')); return; }
        const chunks = [];
        let stdoutBytes = 0;
        let failure = null;
        let closed = false;
        const fail = message => {
            if (closed || failure) return;
            failure = nativeFailure(message);
            // The native validator has no child-process API. Terminate this owned
            // worker, but settle only on close, after its pipes have been reaped.
            child.kill('SIGKILL');
        };
        const abort = () => fail('request was cancelled');
        const timer = setTimeout(() => fail('worker deadline exceeded'), timeoutMs);
        signal?.addEventListener('abort', abort, { once: true });
        child.stdout.on('data', chunk => {
            if (failure || closed) return;
            if (chunk.length > MAX_OUTPUT_BYTES - stdoutBytes) {
                fail('response exceeds size limit');
                return;
            }
            stdoutBytes += chunk.length;
            chunks.push(chunk);
        });
        child.stderr.on('data', chunk => {
            if (chunk.length) fail('worker wrote unexpected diagnostics');
        });
        child.on('error', () => fail('worker could not be started'));
        child.stdout.on('error', () => fail('worker output could not be read'));
        child.stderr.on('error', () => fail('worker diagnostics could not be read'));
        child.stdin.on('error', () => fail('worker input could not be written'));
        child.on('close', (code, signalName) => {
            if (closed) return;
            closed = true;
            clearTimeout(timer);
            signal?.removeEventListener('abort', abort);
            if (failure) { reject(failure); return; }
            if (code !== 0 || signalName) { reject(nativeFailure('worker did not exit successfully')); return; }
            let output;
            try { output = new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks, stdoutBytes)); }
            catch { reject(nativeFailure('response is not valid UTF-8')); return; }
            try { resolve(parseResponse(output)); } catch (error) { reject(error); }
        });
        if (signal?.aborted) abort();
        if (!failure) child.stdin.end(input);
    });
}

function validatePatchPlanNative(request, options = {}) {
    const timeoutMs = options.timeoutMs ?? MAX_TIMEOUT_MS;
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > MAX_TIMEOUT_MS) {
        return Promise.reject(nativeFailure('invalid worker deadline'));
    }
    if (options.signal && (typeof options.signal.addEventListener !== 'function'
        || typeof options.signal.removeEventListener !== 'function')) {
        return Promise.reject(nativeFailure('invalid cancellation signal'));
    }
    if (options.signal?.aborted) return Promise.reject(nativeFailure('request was cancelled'));
    let input;
    try { input = `${JSON.stringify(request)}\n`; } catch { return Promise.reject(nativeFailure('request is not serializable')); }
    if (Buffer.byteLength(input) > MAX_INPUT_BYTES) return Promise.reject(nativeFailure('request exceeds size limit'));

    const executable = sidecarPath(options.sidecarPath);
    if (!executable) return null;
    if (!fs.existsSync(executable)) {
        if (options.sidecarPath) return null;
        return Promise.reject(nativeFailure('worker binary is missing'));
    }
    return runWorker(executable, input, timeoutMs, options.signal);
}

module.exports = {
    validatePatchPlanNative,
    sidecarPath,
    _protocol: { parseResponse, runWorker }
};
