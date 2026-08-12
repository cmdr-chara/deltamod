// Copyright 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const MAX_OUTPUT_BYTES = 8 * 1024;
const TREE_ERROR_CODES = new Set([
    'ARCHIVE_INVALID_LIMIT',
    'ARCHIVE_FILE_LIMIT',
    'ARCHIVE_NESTING_LIMIT',
    'ARCHIVE_LINK_BLOCKED',
    'ARCHIVE_SIZE_LIMIT',
    'ARCHIVE_PATH_ESCAPE',
    'ARCHIVE_ENTRY_TYPE',
    'ARCHIVE_IO_ERROR'
]);

function nativeFailure(message) {
    const error = new Error(`Native extracted-tree validation failed: ${message}`);
    error.code = 'ARCHIVE_NATIVE_FAILED';
    return error;
}

function sidecarPath(override) {
    if (override) return path.resolve(override);
    if (process.platform !== 'win32') return null;

    const executable = 'deltamod-security-worker.exe';
    if (process.resourcesPath) {
        const unpacked = path.join(process.resourcesPath, 'app.asar.unpacked', 'native', 'security-worker', 'bin', `${process.platform}-${process.arch}`, executable);
        if (fs.existsSync(unpacked)) return unpacked;
    }
    const packaged = path.join(__dirname, '..', '..', 'native', 'security-worker', 'bin', `${process.platform}-${process.arch}`, executable);
    if (fs.existsSync(packaged)) return packaged;
    return path.join(__dirname, '..', '..', 'native', 'target', 'debug', executable);
}

function parseResponse(output) {
    if (Buffer.byteLength(output, 'utf8') > MAX_OUTPUT_BYTES) throw nativeFailure('response exceeds size limit');
    if (!output.endsWith('\n') || output.slice(0, -1).includes('\n')) throw nativeFailure('expected one JSON response');
    let response;
    try {
        response = JSON.parse(output);
    } catch {
        throw nativeFailure('response is not valid JSON');
    }
    if (!response || typeof response !== 'object' || Array.isArray(response)) throw nativeFailure('invalid response schema');

    const keys = Object.keys(response).sort();
    if (response.ok === true) {
        if (keys.join(',') !== 'expandedBytes,fileCount,ok'
            || !Number.isSafeInteger(response.fileCount) || response.fileCount < 0
            || !Number.isSafeInteger(response.expandedBytes) || response.expandedBytes < 0) {
            throw nativeFailure('invalid success response');
        }
        return { fileCount: response.fileCount, expandedBytes: response.expandedBytes };
    }
    if (response.ok === false) {
        if (keys.join(',') !== 'code,message,ok' || !TREE_ERROR_CODES.has(response.code)
            || typeof response.message !== 'string' || !response.message || response.message.length > 256) {
            throw nativeFailure('invalid failure response');
        }
        const error = new Error(response.message);
        error.code = response.code;
        throw error;
    }
    throw nativeFailure('invalid response schema');
}

function validateExtractedTreeNative(root, limits, options = {}) {
    if (typeof root !== 'string' || !root || root.includes('\0')) throw nativeFailure('invalid root argument');
    for (const name of ['maxFiles', 'maxExpandedBytes', 'maxDepth']) {
        if (!Number.isSafeInteger(limits[name]) || limits[name] < 0) throw nativeFailure(`invalid ${name} argument`);
    }

    const executable = sidecarPath(options.sidecarPath);
    if (!executable || !fs.existsSync(executable)) return null;

    return new Promise((resolve, reject) => {
        const child = spawn(executable, [
            path.resolve(root),
            String(limits.maxFiles),
            String(limits.maxExpandedBytes),
            String(limits.maxDepth)
        ], {
            windowsHide: true,
            shell: false,
            stdio: ['ignore', 'pipe', 'pipe']
        });
        let stdout = Buffer.alloc(0);
        let stderrBytes = 0;
        let outputTooLarge = false;
        let settled = false;
        const settle = (callback, value) => {
            if (settled) return;
            settled = true;
            callback(value);
        };

        child.stdout.on('data', chunk => {
            if (outputTooLarge) return;
            stdout = Buffer.concat([stdout, chunk]);
            if (stdout.length > MAX_OUTPUT_BYTES) {
                outputTooLarge = true;
                child.kill();
            }
        });
        child.stderr.on('data', chunk => {
            stderrBytes += chunk.length;
            if (stderrBytes > MAX_OUTPUT_BYTES) child.kill();
        });
        child.on('error', error => settle(reject, nativeFailure(error.message)));
        child.on('close', code => {
            if (outputTooLarge || stderrBytes > MAX_OUTPUT_BYTES) {
                settle(reject, nativeFailure('output exceeds size limit'));
                return;
            }
            if (stderrBytes !== 0) {
                settle(reject, nativeFailure('worker wrote unexpected diagnostics'));
                return;
            }
            if (code !== 0) {
                settle(reject, nativeFailure(`worker exited with code ${code}`));
                return;
            }
            try {
                settle(resolve, parseResponse(stdout.toString('utf8')));
            } catch (error) {
                settle(reject, error);
            }
        });
    });
}

module.exports = {
    validateExtractedTreeNative,
    sidecarPath,
    _protocol: { parseResponse }
};
