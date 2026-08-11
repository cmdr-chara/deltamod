// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const MAX_INPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_BYTES = 8 * 1024;
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
    if (process.resourcesPath) {
        const unpacked = path.join(process.resourcesPath, 'app.asar.unpacked', 'native', 'patch-plan-worker', 'bin', 'win32-x64', executable);
        if (fs.existsSync(unpacked)) return unpacked;
    }
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

function validatePatchPlanNative(request, options = {}) {
    let input;
    try { input = `${JSON.stringify(request)}\n`; } catch { return Promise.reject(nativeFailure('request is not serializable')); }
    if (Buffer.byteLength(input) > MAX_INPUT_BYTES) return Promise.reject(nativeFailure('request exceeds size limit'));

    const executable = sidecarPath(options.sidecarPath);
    if (!executable) return null;
    if (!fs.existsSync(executable)) {
        if (options.sidecarPath) return null;
        return Promise.reject(nativeFailure('worker binary is missing'));
    }

    return new Promise((resolve, reject) => {
        const child = spawn(executable, [], {
            windowsHide: true,
            shell: false,
            stdio: ['pipe', 'pipe', 'pipe']
        });
        let stdout = Buffer.alloc(0);
        let stderrBytes = 0;
        let failure = null;
        child.stdout.on('data', chunk => {
            if (failure) return;
            stdout = Buffer.concat([stdout, chunk]);
            if (stdout.length > MAX_OUTPUT_BYTES) {
                failure = nativeFailure('response exceeds size limit');
                child.kill();
            }
        });
        child.stderr.on('data', chunk => {
            stderrBytes += chunk.length;
            if (stderrBytes > MAX_OUTPUT_BYTES) child.kill();
        });
        child.on('error', error => { failure ||= nativeFailure(error.message); });
        child.on('close', code => {
            if (failure) { reject(failure); return; }
            if (stderrBytes !== 0) { reject(nativeFailure('worker wrote unexpected diagnostics')); return; }
            if (code !== 0) { reject(nativeFailure(`worker exited with code ${code}`)); return; }
            try { resolve(parseResponse(stdout.toString('utf8'))); } catch (error) { reject(error); }
        });
        child.stdin.on('error', error => { failure ||= nativeFailure(error.message); });
        child.stdin.end(input);
    });
}

module.exports = {
    validatePatchPlanNative,
    sidecarPath,
    _protocol: { parseResponse }
};
