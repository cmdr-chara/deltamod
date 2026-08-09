const fs = require('fs');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const MAX_INPUT_BYTES = 2 * 1024 * 1024;
const MAX_OUTPUT_BYTES = 8 * 1024;

function sidecarPath(override) {
    if (override) return path.resolve(override);
    if (process.platform !== 'win32' || process.arch !== 'x64') return null;
    const executable = 'deltamod-patch-transaction-worker.exe';
    const candidates = [];
    if (process.resourcesPath) candidates.push(path.join(process.resourcesPath, 'app.asar.unpacked', 'native', 'patch-transaction-worker', 'bin', 'win32-x64', executable));
    return candidates.find(fs.existsSync) || (override ? path.resolve(override) : null);
}

function failure(message) {
    const error = new Error(`Native patch transaction failed: ${message}`);
    error.code = 'PATCH_TRANSACTION_NATIVE_FAILED';
    return error;
}

function requestFor(action, gameRoot, journal, target) {
    const request = { action, game_root: path.resolve(gameRoot), journal, target };
    if (target === undefined) delete request.target;
    const input = `${JSON.stringify(request)}\n`;
    if (Buffer.byteLength(input) > MAX_INPUT_BYTES) throw failure('request exceeds size limit');
    return input;
}

function parse(output) {
    if (output.length > MAX_OUTPUT_BYTES || !output.endsWith('\n') || output.slice(0, -1).includes('\n')) throw failure('expected one bounded JSON response');
    let response;
    try { response = JSON.parse(output); } catch { throw failure('response is not valid JSON'); }
    if (response?.ok === true && Object.keys(response).length === 1) return response;
    if (response?.ok === false && Object.keys(response).sort().join(',') === 'code,message,ok'
        && response.code === 'PATCH_TRANSACTION_INVALID' && typeof response.message === 'string' && response.message.length <= 512) {
        const error = failure(response.message);
        error.code = response.code;
        throw error;
    }
    throw failure('response schema is invalid');
}

function invokeSync(action, gameRoot, journal, options = {}, target) {
    const executable = sidecarPath(options.sidecarPath);
    if (!executable) return null;
    if (!fs.existsSync(executable)) return null;
    const result = spawnSync(executable, [], { input: requestFor(action, gameRoot, journal, target), encoding: 'utf8', windowsHide: true, shell: false, timeout: options.timeoutMs || 30_000 });
    if (result.error) throw failure(result.error.message);
    if (result.stderr) throw failure('worker wrote unexpected diagnostics');
    if (result.status !== 0) throw failure(`worker exited with code ${result.status}`);
    return parse(result.stdout);
}

function invoke(action, gameRoot, journal, options = {}, target) {
    const executable = sidecarPath(options.sidecarPath);
    if (!executable || !fs.existsSync(executable)) return null;
    const input = requestFor(action, gameRoot, journal, target);
    return new Promise((resolve, reject) => {
        const child = spawn(executable, [], { windowsHide: true, shell: false, stdio: ['pipe', 'pipe', 'pipe'] });
        let stdout = ''; let stderr = ''; let settled = false;
        const finish = fn => { if (settled) return; settled = true; fn(); };
        child.stdout.on('data', chunk => { stdout += chunk; if (Buffer.byteLength(stdout) > MAX_OUTPUT_BYTES) { child.kill(); finish(() => reject(failure('response exceeds size limit'))); } });
        child.stderr.on('data', chunk => { stderr += chunk; if (Buffer.byteLength(stderr) > MAX_OUTPUT_BYTES) child.kill(); });
        child.on('error', error => finish(() => reject(failure(error.message))));
        child.on('close', code => finish(() => {
            if (stderr) return reject(failure('worker wrote unexpected diagnostics'));
            if (code !== 0) return reject(failure(`worker exited with code ${code}`));
            try { resolve(parse(stdout)); } catch (error) { reject(error); }
        }));
        child.stdin.end(input);
    });
}

module.exports = { invoke, invokeSync, sidecarPath, _protocol: { parse } };
