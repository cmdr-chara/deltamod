#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const childProcess = require('node:child_process');

const DEFAULTS = Object.freeze({
    timeoutMs: 10_000,
    readinessMs: 1_000,
    pollMs: 25,
    outputLimitBytes: 64 * 1024,
    terminationTimeoutMs: 3_000
});

const LIMITS = Object.freeze({
    timeoutMs: 120_000,
    readinessMs: 120_000,
    pollMs: 1_000,
    outputLimitBytes: 16 * 1024 * 1024,
    terminationTimeoutMs: 30_000
});

const RUNNER_NAME = 'tauri-packaged-smoke';
const DATA_ROOT_ENVIRONMENT_VARIABLE = 'DELTAMOD_SMOKE_DATA_ROOT';
const CAPABILITY_FILE_ENVIRONMENT_VARIABLE = 'DELTAMOD_SMOKE_CAPABILITY_FILE';
const CAPABILITY_FILE = '.deltamod-capability-evidence.json';

class BoundedOutput {
    constructor(limitBytes) {
        this.limitBytes = limitBytes;
        this.chunks = [];
        this.capturedBytes = 0;
        this.totalBytes = 0;
        this.truncated = false;
    }

    append(chunk) {
        const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
        if (buffer.length === 0) return;

        this.totalBytes = Math.min(Number.MAX_SAFE_INTEGER, this.totalBytes + buffer.length);
        const remaining = this.limitBytes - this.capturedBytes;
        if (remaining > 0) {
            const captured = buffer.subarray(0, remaining);
            this.chunks.push(captured);
            this.capturedBytes += captured.length;
        }
        if (buffer.length > Math.max(remaining, 0)) this.truncated = true;
    }

    toJSON() {
        return {
            text: Buffer.concat(this.chunks, this.capturedBytes).toString('utf8'),
            capturedBytes: this.capturedBytes,
            totalBytes: this.totalBytes,
            limitBytes: this.limitBytes,
            truncated: this.truncated
        };
    }
}

function boundedOutput(limitBytes) {
    return new BoundedOutput(limitBytes);
}

function pickOption(options, names) {
    for (const name of names) {
        if (Object.prototype.hasOwnProperty.call(options, name) && options[name] !== undefined) {
            return options[name];
        }
    }
    return undefined;
}

function parseBoundedInteger(value, name, minimum, maximum) {
    const text = String(value);
    if (!/^(?:0|[1-9]\d*)$/.test(text)) {
        throw new Error(`${name} must be a non-negative integer`);
    }
    const number = Number(text);
    if (!Number.isSafeInteger(number) || number < minimum || number > maximum) {
        throw new Error(`${name} must be between ${minimum} and ${maximum}`);
    }
    return number;
}

function normalizeInteger(value, name, fallback, minimum, maximum) {
    if (value === undefined || value === null) return fallback;
    return parseBoundedInteger(value, name, minimum, maximum);
}

function normalizeStringList(value, name) {
    if (value === undefined || value === null) return [];
    if (!Array.isArray(value)) throw new Error(`${name} must be an array`);
    return value.map((item, index) => {
        if (typeof item !== 'string') throw new Error(`${name}[${index}] must be a string`);
        return item;
    });
}

function normalizeOptions(rawOptions) {
    const source = rawOptions && typeof rawOptions === 'object' ? rawOptions : {};
    const executableValue = pickOption(source, ['executable', 'exe'])
        ?? process.env.DELTAMOD_TAURI_EXECUTABLE;
    if (typeof executableValue !== 'string' || executableValue.trim() === '') {
        throw new Error('an explicit --executable path is required');
    }

    const args = normalizeStringList(
        pickOption(source, ['args', 'arguments', 'appArgs']),
        'args'
    );
    const environment = pickOption(source, ['env', 'environment']);
    if (environment !== undefined && (environment === null || typeof environment !== 'object' || Array.isArray(environment))) {
        throw new Error('env must be an object when provided');
    }
    const dataRootValue = pickOption(source, ['dataRoot', 'dataRootPath'])
        ?? environment?.[DATA_ROOT_ENVIRONMENT_VARIABLE]
        ?? process.env[DATA_ROOT_ENVIRONMENT_VARIABLE];
    const dataRoot = dataRootValue === undefined || dataRootValue === null || dataRootValue === ''
        ? null
        : path.resolve(String(dataRootValue));
    const hasDataRootArgument = args.some(argument => (
        argument === '--data-root' || argument.startsWith('--data-root=')
    ));
    const childArgs = dataRoot && !hasDataRootArgument
        ? [...args, '--data-root', dataRoot]
        : [...args];

    const cwdValue = pickOption(source, ['cwd', 'workingDirectory']);
    const cwd = cwdValue === undefined || cwdValue === null || cwdValue === ''
        ? process.cwd()
        : path.resolve(String(cwdValue));

    const env = { ...process.env, ...(environment || {}) };
    if (dataRoot) env[DATA_ROOT_ENVIRONMENT_VARIABLE] = dataRoot;
    const capabilityProbe = pickOption(source, ['capabilityProbe']) === true;
    if (capabilityProbe && !dataRoot) {
        throw new Error('capability probe requires an explicit disposable data root');
    }
    const capabilityFile = capabilityProbe ? path.join(dataRoot, CAPABILITY_FILE) : null;
    if (capabilityFile) env[CAPABILITY_FILE_ENVIRONMENT_VARIABLE] = capabilityFile;
    const expectedVersionValue = pickOption(source, ['expectedVersion']);
    const expectedVersion = expectedVersionValue === undefined || expectedVersionValue === null
        ? null
        : String(expectedVersionValue);
    if (expectedVersion !== null && !/^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(expectedVersion)) {
        throw new Error('expectedVersion must be a stable semantic version');
    }

    const platform = String(pickOption(source, ['platform']) || process.platform);
    return {
        executable: executableValue,
        args,
        childArgs,
        dataRoot,
        capabilityProbe,
        capabilityFile,
        expectedVersion,
        cwd,
        env,
        platform,
        timeoutMs: normalizeInteger(
            pickOption(source, ['timeoutMs', 'timeout', 'startupTimeoutMs', 'readinessTimeoutMs']),
            'timeoutMs',
            DEFAULTS.timeoutMs,
            1,
            LIMITS.timeoutMs
        ),
        readinessMs: normalizeInteger(
            pickOption(source, ['readinessMs', 'readyForMs', 'readyMs', 'holdMs', 'aliveForMs', 'windowMs']),
            'readinessMs',
            DEFAULTS.readinessMs,
            1,
            LIMITS.readinessMs
        ),
        pollMs: normalizeInteger(
            pickOption(source, ['pollMs', 'pollIntervalMs']),
            'pollMs',
            DEFAULTS.pollMs,
            1,
            LIMITS.pollMs
        ),
        outputLimitBytes: normalizeInteger(
            pickOption(source, ['outputLimitBytes', 'maxOutputBytes', 'outputLimit']),
            'outputLimitBytes',
            DEFAULTS.outputLimitBytes,
            0,
            LIMITS.outputLimitBytes
        ),
        terminationTimeoutMs: normalizeInteger(
            pickOption(source, ['terminationTimeoutMs', 'killTimeoutMs']),
            'terminationTimeoutMs',
            DEFAULTS.terminationTimeoutMs,
            1,
            LIMITS.terminationTimeoutMs
        ),
        evidenceFile: pickOption(source, ['evidenceFile', 'evidencePath'])
            ? path.resolve(String(pickOption(source, ['evidenceFile', 'evidencePath'])))
            : null
    };
}

function readCliValue(argv, index, optionName, inlineValue) {
    if (inlineValue !== undefined) {
        if (inlineValue === '') throw new Error(`${optionName} requires a value`);
        return { value: inlineValue, nextIndex: index };
    }
    const next = argv[index + 1];
    if (next === undefined || next === '--' || next.startsWith('--')) {
        throw new Error(`${optionName} requires a value`);
    }
    return { value: next, nextIndex: index + 1 };
}

function parseCli(argv = process.argv.slice(2)) {
    const raw = { args: [] };
    let passThrough = false;
    let help = false;

    const valueOptions = new Map([
        ['--executable', 'executable'],
        ['--data-root', 'dataRoot'],
        ['--timeout-ms', 'timeoutMs'],
        ['--timeout', 'timeoutMs'],
        ['--startup-timeout-ms', 'timeoutMs'],
        ['--startup-timeout', 'timeoutMs'],
        ['--readiness-timeout-ms', 'timeoutMs'],
        ['--readiness-timeout', 'timeoutMs'],
        ['--ready-for-ms', 'readinessMs'],
        ['--ready-for', 'readinessMs'],
        ['--readiness-ms', 'readinessMs'],
        ['--readiness', 'readinessMs'],
        ['--ready-ms', 'readinessMs'],
        ['--hold-ms', 'readinessMs'],
        ['--alive-for-ms', 'readinessMs'],
        ['--alive-ms', 'readinessMs'],
        ['--liveness-ms', 'readinessMs'],
        ['--window-ms', 'readinessMs'],
        ['--window-duration-ms', 'readinessMs'],
        ['--poll-ms', 'pollMs'],
        ['--poll-interval-ms', 'pollMs'],
        ['--output-limit-bytes', 'outputLimitBytes'],
        ['--max-output-bytes', 'outputLimitBytes'],
        ['--output-limit', 'outputLimitBytes'],
        ['--termination-timeout-ms', 'terminationTimeoutMs'],
        ['--kill-timeout-ms', 'terminationTimeoutMs'],
        ['--cwd', 'cwd'],
        ['--evidence-file', 'evidenceFile'],
        ['--evidence', 'evidenceFile'],
        ['--expected-version', 'expectedVersion']
    ]);

    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index];
        if (passThrough) {
            raw.args.push(argument);
            continue;
        }
        if (argument === '--') {
            passThrough = true;
            continue;
        }
        if (argument === '--help' || argument === '-h') {
            help = true;
            continue;
        }
        if (argument === '--json') continue;
        if (argument === '--capability-probe') {
            raw.capabilityProbe = true;
            continue;
        }

        const separator = argument.indexOf('=');
        const optionName = separator === -1 ? argument : argument.slice(0, separator);
        const inlineValue = separator === -1 ? undefined : argument.slice(separator + 1);
        const property = valueOptions.get(optionName);
        if (!property) throw new Error(`unknown option ${argument}; app arguments must follow --`);

        const read = readCliValue(argv, index, optionName, inlineValue);
        raw[property] = read.value;
        index = read.nextIndex;
    }

    if (help) return { help: true };
    return normalizeOptions(raw);
}

function monotonicMilliseconds(start) {
    return Number(process.hrtime.bigint() - start) / 1_000_000;
}

function roundedMilliseconds(value) {
    return Math.max(0, Math.round(value));
}

function errorRecord(error) {
    return {
        code: typeof error?.code === 'string' ? error.code : null,
        message: error instanceof Error ? error.message : String(error)
    };
}

function emptyStreamEvidence(limitBytes) {
    return {
        text: '',
        capturedBytes: 0,
        totalBytes: 0,
        limitBytes,
        truncated: false
    };
}

function createEvidence(options) {
    return {
        schemaVersion: 1,
        runner: RUNNER_NAME,
        status: 'failed',
        ok: false,
        command: {
            executable: options.executable,
            args: [...options.childArgs],
            cwd: options.cwd,
            dataRoot: options.dataRoot
        },
        configuration: {
            timeoutMs: options.timeoutMs,
            readinessMs: options.readinessMs,
            pollMs: options.pollMs,
            outputLimitBytes: options.outputLimitBytes,
            terminationTimeoutMs: options.terminationTimeoutMs,
            platform: options.platform,
            readinessCriterion: options.capabilityProbe ? 'capability-evidence-and-process-live' : 'process-live'
        },
        capability: {
            required: Boolean(options.capabilityProbe),
            file: options.capabilityFile || null,
            observed: false,
            evidence: null
        },
        readiness: {
            criterion: options.capabilityProbe ? 'capability-evidence-and-process-live' : 'process-live',
            reached: false,
            requiredForMs: options.readinessMs,
            firstObservedAfterMs: null,
            observedForMs: 0,
            checks: 0
        },
        isolation: {
            dataRootRequired: Boolean(options.dataRoot),
            dataRootObserved: false,
            entryCount: 0
        },
        process: {
            pid: null,
            spawnAttempted: false,
            spawned: false,
            exited: false,
            exitCode: null,
            signal: null,
            error: null,
            closed: false
        },
        output: {
            stdout: emptyStreamEvidence(options.outputLimitBytes),
            stderr: emptyStreamEvidence(options.outputLimitBytes)
        },
        termination: {
            requested: false,
            tree: true,
            method: 'none',
            graceful: false,
            forced: false,
            completed: false,
            timedOut: false,
            error: null
        },
        failure: null,
        durationMs: 0
    };
}

function createCliFailureEvidence(error) {
    const options = {
        executable: null,
        childArgs: [],
        cwd: process.cwd(),
        dataRoot: null,
        timeoutMs: DEFAULTS.timeoutMs,
        readinessMs: DEFAULTS.readinessMs,
        pollMs: DEFAULTS.pollMs,
        outputLimitBytes: DEFAULTS.outputLimitBytes,
        terminationTimeoutMs: DEFAULTS.terminationTimeoutMs,
        platform: process.platform
    };
    const evidence = createEvidence(options);
    evidence.failure = { code: 'invalid-cli', message: errorRecord(error).message };
    evidence.process.spawnAttempted = false;
    evidence.termination.completed = true;
    return evidence;
}

function setFailure(evidence, code, message) {
    evidence.status = 'failed';
    evidence.ok = false;
    if (!evidence.failure) evidence.failure = { code, message };
}

function inspectDataRoot(dataRoot) {
    if (!dataRoot) return { dataRootRequired: false, dataRootObserved: false, entryCount: 0 };
    const metadata = fs.lstatSync(dataRoot);
    if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
        throw new Error('disposable data root is not a regular directory');
    }
    const entries = fs.readdirSync(dataRoot);
    return {
        dataRootRequired: true,
        dataRootObserved: entries.length > 0,
        entryCount: entries.length
    };
}

function inspectCapabilityEvidence(options) {
    if (!options.capabilityProbe) {
        return { required: false, file: null, observed: false, evidence: null };
    }
    const metadata = fs.lstatSync(options.capabilityFile);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > 64 * 1024) {
        throw new Error('capability evidence is not a bounded regular file');
    }
    const evidence = JSON.parse(fs.readFileSync(options.capabilityFile, 'utf8'));
    if (evidence?.schemaVersion !== 1 || evidence?.ok !== true || evidence?.status !== 'passed') {
        throw new Error(`capability probe failed: ${String(evidence?.error || 'invalid evidence')}`);
    }
    const requiredChecks = [
        'packaged', 'flagSet', 'flagRead', 'flagPersisted', 'baseThemeAvailable',
        'baseThemeActive', 'installationListed', 'gameLoaded', 'unknownChannelRejected'
    ];
    if (requiredChecks.some(check => evidence.checks?.[check] !== true)) {
        throw new Error('capability evidence is missing a required passing check');
    }
    if (options.expectedVersion !== null && evidence.packageVersion !== options.expectedVersion) {
        throw new Error(`capability evidence reports ${String(evidence.packageVersion)} instead of ${options.expectedVersion}`);
    }
    return { required: true, file: options.capabilityFile, observed: true, evidence };
}

function isProcessAlive(pid) {
    if (!Number.isInteger(pid) || pid <= 0) return false;
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error?.code === 'EPERM';
    }
}

function childHasExited(child) {
    return Boolean(child && (
        typeof child.exitCode === 'number'
        || typeof child.signalCode === 'string'
    ));
}

function isChildAlive(child) {
    if (!child || !Number.isInteger(child.pid)) return false;
    if (childHasExited(child)) return false;
    return isProcessAlive(child.pid);
}

function waitForReadiness(child, options, start, state) {
    return new Promise(resolve => {
        let finished = false;
        let timer = null;
        let deadlineTimer = null;
        let firstObservedAt = null;
        let checks = 0;

        const cleanup = () => {
            if (timer) clearTimeout(timer);
            if (deadlineTimer) clearTimeout(deadlineTimer);
            child.removeListener('spawn', onSpawn);
            child.removeListener('exit', onExit);
            child.removeListener('error', onError);
        };

        const finish = (passed, code, message) => {
            if (finished) return;
            finished = true;
            cleanup();
            const observedForMs = firstObservedAt === null
                ? 0
                : roundedMilliseconds(monotonicMilliseconds(start) - firstObservedAt);
            resolve({
                passed,
                code,
                message,
                reached: passed,
                requiredForMs: options.readinessMs,
                firstObservedAfterMs: firstObservedAt === null
                    ? null
                    : roundedMilliseconds(firstObservedAt),
                observedForMs,
                checks
            });
        };

        const failForExit = () => finish(
            false,
            'process-exited-before-readiness',
            `packaged executable did not remain alive for ${options.readinessMs} ms`
        );

        const onSpawn = () => {
            state.spawned = true;
            check();
        };

        const onExit = () => failForExit();

        const onError = error => finish(
            false,
            'spawn-error',
            `failed to start packaged executable: ${errorRecord(error).message}`
        );

        const check = () => {
            if (finished) return;
            checks += 1;

            if (state.error) {
                onError(state.error);
                return;
            }
            if (state.exited || childHasExited(child)) {
                failForExit();
                return;
            }

            const elapsed = monotonicMilliseconds(start);
            if (!Number.isInteger(child.pid)) {
                if (elapsed >= options.timeoutMs) {
                    finish(false, 'readiness-timeout', `packaged executable was not observed within ${options.timeoutMs} ms`);
                } else {
                    timer = setTimeout(check, Math.min(options.pollMs, options.timeoutMs - elapsed));
                }
                return;
            }

            state.spawned = true;
            if (!isChildAlive(child)) {
                if (elapsed >= options.timeoutMs) {
                    finish(false, 'process-not-alive-before-readiness', 'packaged executable was not alive when readiness was checked');
                } else {
                    timer = setTimeout(check, Math.min(options.pollMs, options.timeoutMs - elapsed));
                }
                return;
            }

            if (options.capabilityProbe && !fs.existsSync(options.capabilityFile)) {
                if (elapsed >= options.timeoutMs) {
                    finish(false, 'capability-timeout', `capability evidence was not observed within ${options.timeoutMs} ms`);
                } else {
                    timer = setTimeout(check, Math.min(options.pollMs, options.timeoutMs - elapsed));
                }
                return;
            }

            if (firstObservedAt === null) {
                firstObservedAt = elapsed;
                state.firstObservedAfterMs = roundedMilliseconds(elapsed);
            }
            const observedFor = elapsed - firstObservedAt;
            if (observedFor >= options.readinessMs) {
                finish(true, null, null);
                return;
            }
            const remainingReadiness = options.readinessMs - observedFor;
            const remainingTimeout = options.timeoutMs - elapsed;
            timer = setTimeout(check, Math.max(1, Math.min(
                options.pollMs,
                remainingReadiness,
                remainingTimeout
            )));
        };

        child.once('spawn', onSpawn);
        child.once('exit', onExit);
        child.once('error', onError);
        deadlineTimer = setTimeout(() => {
            finish(
                false,
                'readiness-timeout',
                `packaged executable did not remain alive for ${options.readinessMs} ms before the ${options.timeoutMs} ms timeout`
            );
        }, options.timeoutMs);
        check();
    });
}

function childIsClosed(child, state) {
    if (!child) return false;
    if (state?.closed) return true;
    return !child.stdout && !child.stderr && childHasExited(child);
}

function waitForChildClose(child, timeoutMs, state) {
    if (!child || childIsClosed(child, state)) return Promise.resolve(true);
    return new Promise(resolve => {
        let finished = false;
        const onClose = () => {
            if (state) state.closed = true;
            finish(true);
        };
        const finish = closed => {
            if (finished) return;
            finished = true;
            clearTimeout(timer);
            child.removeListener('close', onClose);
            resolve(closed);
        };
        const timer = setTimeout(() => finish(false), Math.max(1, timeoutMs));
        child.once('close', onClose);
        if (childIsClosed(child, state)) finish(true);
    });
}

function isProcessGroupAlive(pid) {
    if (!Number.isInteger(pid) || pid <= 0) return false;
    try {
        process.kill(-pid, 0);
        return true;
    } catch (error) {
        return error?.code === 'EPERM';
    }
}

async function waitForProcessGroupTermination(pid, child, state, timeoutMs) {
    const deadline = Date.now() + Math.max(1, timeoutMs);
    while (Date.now() <= deadline) {
        if (childIsClosed(child, state) && !isProcessGroupAlive(pid)) return true;
        await new Promise(resolve => setTimeout(resolve, Math.min(25, remainingMilliseconds(deadline))));
    }
    return childIsClosed(child, state) && !isProcessGroupAlive(pid);
}

function remainingMilliseconds(deadline) {
    return Math.max(1, deadline - Date.now());
}

function runTaskkill(pid, timeoutMs, spawnImpl = childProcess.spawn) {
    return new Promise(resolve => {
        let finished = false;
        let command;
        let timer = null;

        const finish = result => {
            if (finished) return;
            finished = true;
            if (timer) clearTimeout(timer);
            resolve(result);
        };

        try {
            command = spawnImpl(
                'taskkill',
                ['/PID', String(pid), '/T', '/F'],
                { shell: false, windowsHide: true, stdio: 'ignore' }
            );
        } catch (error) {
            finish({ ok: false, timedOut: false, error: errorRecord(error) });
            return;
        }

        command.once('error', error => finish({
            ok: false,
            timedOut: false,
            error: errorRecord(error)
        }));
        command.once('exit', (code, signal) => finish({
            ok: code === 0,
            timedOut: false,
            error: code === 0
                ? null
                : { code: 'TASKKILL_FAILED', message: `taskkill exited with code ${code}${signal ? ` (${signal})` : ''}` }
        }));
        timer = setTimeout(() => {
            try {
                command.kill();
            } catch {
                // The command is already gone or cannot be signalled; the bounded result remains a failure.
            }
            command.unref?.();
            finish({
                ok: false,
                timedOut: true,
                error: { code: 'TASKKILL_TIMEOUT', message: `taskkill exceeded ${timeoutMs} ms` }
            });
        }, Math.max(1, timeoutMs));
    });
}

async function terminateProcessTree(child, rawOptions = {}) {
    const platform = String(rawOptions.platform || process.platform);
    const timeoutMs = normalizeInteger(
        rawOptions.timeoutMs,
        'terminationTimeoutMs',
        DEFAULTS.terminationTimeoutMs,
        1,
        LIMITS.terminationTimeoutMs
    );
    const pid = Number.isInteger(child?.pid) ? child.pid : null;
    const result = {
        requested: pid !== null,
        tree: true,
        method: 'none',
        graceful: false,
        forced: false,
        completed: pid === null,
        timedOut: false,
        error: null
    };
    if (pid === null) return result;

    const deadline = Date.now() + timeoutMs;
    if (platform === 'win32') {
        const alreadyExited = Boolean(rawOptions.state?.exited || childHasExited(child));
        if (alreadyExited) {
            const closed = rawOptions.state?.closed
                || await waitForChildClose(child, remainingMilliseconds(deadline), rawOptions.state);
            result.method = 'none';
            result.completed = closed;
            if (!closed) {
                result.timedOut = true;
                result.error = `process tree did not terminate within ${timeoutMs} ms`;
            }
            return result;
        }
        result.method = 'taskkill';
        const taskkill = await runTaskkill(pid, remainingMilliseconds(deadline), rawOptions.spawnImpl);
        if (taskkill.ok) {
            result.forced = true;
        } else {
            result.method = 'child-kill-fallback';
            result.error = taskkill.error?.message || 'taskkill failed';
            result.timedOut = taskkill.timedOut;
            try {
                result.graceful = child.kill();
            } catch (error) {
                result.error = `${result.error}; ${errorRecord(error).message}`;
            }
        }
        const closed = await waitForChildClose(child, remainingMilliseconds(deadline), rawOptions.state);
        const noLongerAlive = !isProcessAlive(pid);
        result.completed = closed && (taskkill.ok || alreadyExited || noLongerAlive);
        if (result.completed && !taskkill.ok && noLongerAlive) {
            result.method = 'already-exited';
            result.error = null;
            result.timedOut = false;
        }
        if (!closed) {
            result.timedOut = true;
            result.error ||= `process tree did not terminate within ${timeoutMs} ms`;
        }
        return result;
    }

    result.method = 'process-group';
    try {
        process.kill(-pid, 'SIGTERM');
        result.graceful = true;
    } catch (error) {
        if (error?.code !== 'ESRCH') result.error = errorRecord(error).message;
        try {
            result.graceful ||= child.kill('SIGTERM');
        } catch (fallbackError) {
            result.error ||= errorRecord(fallbackError).message;
        }
    }

    let closed = await waitForProcessGroupTermination(
        pid,
        child,
        rawOptions.state,
        remainingMilliseconds(deadline)
    );
    if (!closed) {
        result.forced = true;
        try {
            process.kill(-pid, 'SIGKILL');
        } catch (error) {
            if (error?.code !== 'ESRCH') result.error ||= errorRecord(error).message;
            try {
                child.kill('SIGKILL');
            } catch (fallbackError) {
                result.error ||= errorRecord(fallbackError).message;
            }
        }
        closed = await waitForProcessGroupTermination(
            pid,
            child,
            rawOptions.state,
            remainingMilliseconds(deadline)
        );
    }
    result.completed = closed;
    if (!closed) {
        result.timedOut = true;
        result.error ||= `process tree did not terminate within ${timeoutMs} ms`;
    }
    return result;
}

function finalizeProcessEvidence(evidence, child, state) {
    if (!child) return;
    evidence.process.pid = Number.isInteger(child.pid) ? child.pid : null;
    evidence.process.spawned = Boolean(state.spawned || child.pid);
    evidence.process.exited = Boolean(state.exited || child.exitCode !== null || child.signalCode !== null);
    evidence.process.exitCode = state.exitCode ?? (typeof child.exitCode === 'number' ? child.exitCode : null);
    evidence.process.signal = state.signal ?? (typeof child.signalCode === 'string' ? child.signalCode : null);
    evidence.process.error = state.error ? errorRecord(state.error) : null;
    evidence.process.closed = childIsClosed(child, state);
}

async function runPackagedSmoke(rawOptions) {
    const options = normalizeOptions(rawOptions);
    const start = process.hrtime.bigint();
    const evidence = createEvidence(options);
    const stdout = boundedOutput(options.outputLimitBytes);
    const stderr = boundedOutput(options.outputLimitBytes);
    const state = {
        spawned: false,
        exited: false,
        exitCode: null,
        signal: null,
        error: null,
        closed: false,
        firstObservedAfterMs: null
    };
    let child = null;

    try {
        evidence.process.spawnAttempted = true;
        child = childProcess.spawn(options.executable, options.childArgs, {
            cwd: options.cwd,
            env: options.env,
            detached: options.platform !== 'win32',
            shell: false,
            windowsHide: true,
            stdio: ['ignore', 'pipe', 'pipe']
        });
        evidence.process.pid = Number.isInteger(child.pid) ? child.pid : null;
        child.once('spawn', () => { state.spawned = true; });
        child.once('error', error => { state.error = error; });
        child.once('exit', (code, signal) => {
            state.exited = true;
            state.exitCode = typeof code === 'number' ? code : null;
            state.signal = typeof signal === 'string' ? signal : null;
        });
        child.once('close', () => { state.closed = true; });
        child.stdout?.on('data', chunk => stdout.append(chunk));
        child.stderr?.on('data', chunk => stderr.append(chunk));

        const readiness = await waitForReadiness(child, options, start, state);
        evidence.readiness = {
            criterion: options.capabilityProbe ? 'capability-evidence-and-process-live' : 'process-live',
            reached: readiness.reached,
            requiredForMs: readiness.requiredForMs,
            firstObservedAfterMs: readiness.firstObservedAfterMs,
            observedForMs: readiness.observedForMs,
            checks: readiness.checks
        };
        if (readiness.passed) {
            evidence.status = 'passed';
            evidence.ok = true;
        } else {
            setFailure(evidence, readiness.code, readiness.message);
        }
    } catch (error) {
        setFailure(evidence, 'runner-error', errorRecord(error).message);
    } finally {
        if (child) {
            try {
                evidence.termination = await terminateProcessTree(child, {
                    platform: options.platform,
                    timeoutMs: options.terminationTimeoutMs,
                    state
                });
            } catch (error) {
                evidence.termination = {
                    ...evidence.termination,
                    requested: Number.isInteger(child.pid),
                    method: options.platform === 'win32' ? 'taskkill' : 'process-group',
                    error: errorRecord(error).message,
                    completed: false
                };
            }
            if (!evidence.termination.completed && !evidence.failure) {
                setFailure(evidence, 'termination-failed', evidence.termination.error || 'packaged process tree could not be terminated');
            }
            if (!evidence.termination.completed) {
                child.stdout?.destroy();
                child.stderr?.destroy();
                child.unref?.();
            }
        } else {
            evidence.termination.completed = true;
        }
        finalizeProcessEvidence(evidence, child, state);
        evidence.output = { stdout: stdout.toJSON(), stderr: stderr.toJSON() };
        try {
            evidence.isolation = inspectDataRoot(options.dataRoot);
            if (options.dataRoot && !evidence.isolation.dataRootObserved) {
                setFailure(
                    evidence,
                    'data-root-unused',
                    'packaged executable did not initialize the disposable data root'
                );
            }
        } catch (error) {
            setFailure(evidence, 'data-root-invalid', errorRecord(error).message);
        }
        try {
            evidence.capability = inspectCapabilityEvidence(options);
        } catch (error) {
            setFailure(evidence, 'capability-probe-failed', errorRecord(error).message);
        }
        evidence.durationMs = roundedMilliseconds(monotonicMilliseconds(start));
    }
    return evidence;
}

function sortedForJson(value) {
    if (Array.isArray(value)) return value.map(sortedForJson);
    if (!value || typeof value !== 'object') return value;
    return Object.fromEntries(
        Object.keys(value).sort().map(key => [key, sortedForJson(value[key])])
    );
}

function stableStringify(value) {
    return JSON.stringify(sortedForJson(value), null, 2);
}

function usage() {
    return [
        'Usage: node scripts/tauri-parity/run-packaged-smoke.js --executable <path> [options] [-- <app arguments>]',
        '',
        'Options:',
        '  --data-root <path>              pass a disposable data root to the app',
        '  --timeout-ms <ms>               bounded time to reach readiness (max 120000)',
        '  --ready-for-ms <ms>             require a live process for this interval',
        '  --poll-ms <ms>                  process liveness polling interval',
        '  --output-limit-bytes <bytes>    cap each captured output stream',
        '  --termination-timeout-ms <ms>  bounded process-tree termination wait',
        '  --cwd <path>                    working directory for the packaged app',
        '  --evidence-file <path>          also write the stable JSON evidence here',
        '  --capability-probe              require in-app Rust capability evidence',
        '  --expected-version <x.y.z>      require the exact packaged application version',
        '',
        'The command emits one JSON evidence document. App arguments must follow --.'
    ].join('\n');
}

async function main(argv = process.argv.slice(2)) {
    let parsed;
    let evidence;
    try {
        parsed = parseCli(argv);
        if (parsed.help) {
            process.stdout.write(`${usage()}\n`);
            return 0;
        }
        evidence = await runPackagedSmoke(parsed);
    } catch (error) {
        evidence = createCliFailureEvidence(error);
    }

    let serialized = stableStringify(evidence);
    if (parsed?.evidenceFile) {
        try {
            fs.writeFileSync(parsed.evidenceFile, `${serialized}\n`, 'utf8');
        } catch (error) {
            setFailure(evidence, 'evidence-write-failed', `could not write evidence file: ${errorRecord(error).message}`);
            serialized = stableStringify(evidence);
        }
    }
    process.stdout.write(`${serialized}\n`);
    return evidence.ok ? 0 : 1;
}

if (require.main === module) {
    main().then(code => {
        process.exitCode = code;
    }).catch(error => {
        const evidence = createCliFailureEvidence(error);
        process.stdout.write(`${stableStringify(evidence)}\n`);
        process.exitCode = 1;
    });
}

module.exports = {
    BoundedOutput,
    DEFAULTS,
    LIMITS,
    createEvidence,
    isProcessAlive,
    normalizeOptions,
    parseCli,
    runPackagedSmoke,
    stableStringify,
    terminateProcessTree,
    usage
};
