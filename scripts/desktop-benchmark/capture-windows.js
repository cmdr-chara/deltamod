// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { performance } = require('node:perf_hooks');
const childProcess = require('node:child_process');

const DEFAULT_PROTOCOL = Object.freeze({
    warmupLaunches: 1,
    measuredLaunches: 7,
    profilePolicy: 'fresh-per-launch',
    fileCache: 'warm',
    readiness: 'first-window-and-main-route-guard-cleared',
    memorySampleWindowMs: 2000,
    memorySampleIntervalMs: 100,
    memoryAggregation: 'sum-of-application-process-working-sets'
});

const DEFAULT_LIMITS = Object.freeze({
    readinessTimeoutMs: 30_000,
    readinessPollIntervalMs: 100,
    readinessCommandTimeoutMs: 5_000,
    processQueryTimeoutMs: 5_000,
    maxProcessCount: 256,
    maxProcessDepth: 32,
    maxArtifactFiles: 200_000,
    maxArtifactBytes: 4 * 1024 * 1024 * 1024 * 1024,
    processQueryMaxBuffer: 1024 * 1024,
    readinessCommandMaxBuffer: 64 * 1024,
    powershellCommand: 'pwsh.exe'
});

const PROCESS_SNAPSHOT_SCRIPT = [
    '$ErrorActionPreference = \'Stop\'',
    'Get-CimInstance -ClassName Win32_Process |',
    '  Select-Object ProcessId, ParentProcessId, WorkingSetSize |',
    '  ConvertTo-Json -Compress'
].join('\n');

function positiveFiniteNumber(value, label) {
    if (!Number.isFinite(value) || value <= 0) {
        throw new Error(`${label} must be a positive finite number`);
    }
    return value;
}

function positiveInteger(value, label) {
    positiveFiniteNumber(value, label);
    if (!Number.isSafeInteger(value)) {
        throw new Error(`${label} must be a positive safe integer`);
    }
    return value;
}

function nonNegativeSafeInteger(value, label) {
    const number = typeof value === 'string' && value.trim() !== ''
        ? Number(value)
        : value;
    if (!Number.isSafeInteger(number) || number < 0) {
        throw new Error(`${label} must be a non-negative safe integer`);
    }
    return number;
}

function requireNonEmptyString(value, label) {
    if (typeof value !== 'string' || value.trim() === '') {
        throw new Error(`${label} is required`);
    }
    return value;
}

function roundMiB(bytes) {
    return Math.round((bytes / 1024 / 1024) * 100) / 100;
}

function median(values) {
    if (!Array.isArray(values) || values.length === 0) {
        throw new Error('cannot calculate a median of an empty sample set');
    }
    const sorted = [...values].sort((left, right) => left - right);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? (sorted[middle - 1] + sorted[middle]) / 2
        : sorted[middle];
}

function nearestRank(values, percentile) {
    if (!Array.isArray(values) || values.length === 0) {
        throw new Error('cannot calculate a percentile of an empty sample set');
    }
    if (!Number.isFinite(percentile) || percentile <= 0 || percentile > 1) {
        throw new Error('percentile must be greater than zero and no greater than one');
    }
    const sorted = [...values].sort((left, right) => left - right);
    return sorted[Math.ceil(percentile * sorted.length) - 1];
}

function summarizeSamples(samples) {
    if (!Array.isArray(samples) || samples.length !== DEFAULT_PROTOCOL.measuredLaunches) {
        throw new Error(
            `expected ${DEFAULT_PROTOCOL.measuredLaunches} measured samples`
        );
    }

    const readyValues = samples.map((sample, index) => positiveFiniteNumber(
        sample?.readyMs,
        `samples[${index}].readyMs`
    ));
    const workingSetValues = samples.map((sample, index) => positiveFiniteNumber(
        sample?.peakWorkingSetBytes,
        `samples[${index}].peakWorkingSetBytes`
    ));
    const readyP95 = nearestRank(readyValues, 0.95);
    const workingSetP95 = nearestRank(workingSetValues, 0.95);

    return {
        readyMs: {
            minimum: Math.min(...readyValues),
            median: median(readyValues),
            p95NearestRank: readyP95
        },
        peakWorkingSetBytes: {
            median: median(workingSetValues),
            medianMiB: roundMiB(median(workingSetValues)),
            p95NearestRank: workingSetP95,
            p95MiB: roundMiB(workingSetP95)
        }
    };
}

function parseCommandConfig(value, label = 'readiness command') {
    let command;
    let args;

    if (typeof value === 'string') {
        command = value;
        args = [];
    } else if (Array.isArray(value)) {
        [command, ...args] = value;
    } else if (value && typeof value === 'object') {
        command = value.command ?? value.executable;
        args = value.args ?? [];
    }

    requireNonEmptyString(command, `${label} executable`);
    if (!Array.isArray(args) || args.some(argument => typeof argument !== 'string')) {
        throw new Error(`${label} args must be an array of strings`);
    }

    return {
        command,
        args: [...args]
    };
}

function replaceReadinessPlaceholders(argument, context) {
    const placeholders = {
        pid: context.pid,
        profile: context.profilePath,
        executable: context.executablePath,
        artifact: context.artifactPath,
        run: context.runIndex,
        measured: context.measured ? 'true' : 'false',
        readiness: context.readiness,
        readyFile: context.readyFile
    };
    return argument.replace(/\{(pid|profile|executable|artifact|run|measured|readiness|readyFile)\}/g,
        (_match, name) => String(placeholders[name]));
}

function execFileAsync(execFileImpl, file, args, options = {}) {
    return new Promise((resolve, reject) => {
        let settled = false;
        const finish = (callback, value) => {
            if (settled) return;
            settled = true;
            callback(value);
        };
        const callback = (error, stdout, stderr) => {
            if (error) {
                error.stdout = stdout;
                error.stderr = stderr;
                finish(reject, error);
                return;
            }
            finish(resolve, { stdout: stdout ?? '', stderr: stderr ?? '' });
        };

        try {
            execFileImpl(file, args, options, callback);
        } catch (error) {
            finish(reject, error);
        }
    });
}

function parseReadinessOutput(stdout) {
    const text = String(stdout ?? '').trim();
    if (!text) return true;

    try {
        const parsed = JSON.parse(text);
        if (typeof parsed === 'boolean') return parsed;
        if (parsed && typeof parsed.ready === 'boolean') return parsed.ready;
    } catch {
        // A successful command is itself the configured readiness signal. Non-JSON
        // diagnostic output is therefore intentionally ignored.
    }
    return true;
}

function createReadinessProbe(commandValue, {
    execFileImpl = childProcess.execFile,
    commandTimeoutMs = DEFAULT_LIMITS.readinessCommandTimeoutMs,
    maxBuffer = DEFAULT_LIMITS.readinessCommandMaxBuffer,
    inheritedEnvironment = process.env
} = {}) {
    const command = parseCommandConfig(commandValue);
    positiveInteger(commandTimeoutMs, 'readiness command timeout');

    return async context => {
        const args = command.args.map(argument => replaceReadinessPlaceholders(argument, context));
        const environment = {
            ...inheritedEnvironment,
            ...(context.environment || {}),
            DELTAMOD_BENCHMARK_PID: String(context.pid),
            DELTAMOD_BENCHMARK_PROFILE: context.profilePath,
            DELTAMOD_BENCHMARK_EXECUTABLE: context.executablePath,
            DELTAMOD_BENCHMARK_ARTIFACT: context.artifactPath,
            DELTAMOD_BENCHMARK_RUN: String(context.runIndex),
            DELTAMOD_BENCHMARK_READINESS: context.readiness
        };

        try {
            const result = await execFileAsync(execFileImpl, command.command, args, {
                env: environment,
                windowsHide: true,
                timeout: commandTimeoutMs,
                maxBuffer
            });
            return parseReadinessOutput(result.stdout);
        } catch (error) {
            if (error?.code === 'ENOENT') {
                throw new Error(`readiness command was not found: ${command.command}`);
            }
            if (typeof error?.code === 'number' || error?.code === 'ETIMEDOUT') {
                return false;
            }
            throw new Error(`readiness command failed: ${error?.message || String(error)}`);
        }
    };
}

function createReadinessFileProbe(fileValue, { cwd = process.cwd() } = {}) {
    const fileTemplate = requireNonEmptyString(fileValue, 'readiness file');
    const baseDirectory = path.resolve(cwd);

    return async context => {
        const resolvedPath = path.resolve(
            baseDirectory,
            replaceReadinessPlaceholders(fileTemplate, context)
        );
        try {
            return fs.statSync(resolvedPath).isFile();
        } catch (error) {
            if (error?.code === 'ENOENT') return false;
            throw new Error(`unable to inspect readiness file ${resolvedPath}: ${error.message}`);
        }
    };
}

function childHasExited(child) {
    return child && (
        (child.exitCode !== null && child.exitCode !== undefined) ||
        child.signalCode !== null && child.signalCode !== undefined
    );
}

function defaultChildIsAlive(child) {
    return !childHasExited(child);
}

async function waitForReadiness({
    child,
    probe,
    context,
    timeoutMs = DEFAULT_LIMITS.readinessTimeoutMs,
    pollIntervalMs = DEFAULT_LIMITS.readinessPollIntervalMs,
    now = () => performance.now(),
    sleep = defaultSleep,
    isAlive = defaultChildIsAlive
}) {
    if (!child || typeof child !== 'object') {
        throw new Error('readiness requires a spawned child process');
    }
    if (typeof probe !== 'function') {
        throw new Error('readiness probe is required; no implicit readiness signal is allowed');
    }
    positiveInteger(timeoutMs, 'readiness timeout');
    positiveInteger(pollIntervalMs, 'readiness poll interval');

    const deadline = now() + timeoutMs;
    while (true) {
        if (!isAlive(child)) {
            throw new Error('application exited before configured readiness was observed');
        }

        if (await probe(context)) {
            const observedAt = now();
            if (observedAt <= deadline) return observedAt;
            throw new Error(
                `configured readiness was not observed within ${timeoutMs} ms`
            );
        }

        const remaining = deadline - now();
        if (remaining <= 0) {
            throw new Error(
                `configured readiness was not observed within ${timeoutMs} ms`
            );
        }
        await sleep(Math.min(pollIntervalMs, remaining));
    }
}

function parseProcessSnapshot(stdout) {
    let parsed;
    try {
        parsed = JSON.parse(String(stdout ?? '').trim());
    } catch (error) {
        throw new Error(`unable to parse Windows process snapshot: ${error.message}`);
    }

    const records = Array.isArray(parsed) ? parsed : [parsed];
    if (records.length === 0 || records.some(record => !record || typeof record !== 'object')) {
        throw new Error('Windows process snapshot was empty or malformed');
    }

    return records.map((record, index) => {
        const pid = nonNegativeSafeInteger(
            record.ProcessId ?? record.processId ?? record.pid,
            `process snapshot[${index}].pid`
        );
        const parentPid = nonNegativeSafeInteger(
            record.ParentProcessId ?? record.parentProcessId ?? record.parentPid,
            `process snapshot[${index}].parentPid`
        );
        const workingSetBytes = nonNegativeSafeInteger(
            record.WorkingSetSize ?? record.workingSetSize ?? record.workingSetBytes,
            `process snapshot[${index}].workingSetBytes`
        );
        return { pid, parentPid, workingSetBytes };
    });
}

function collectProcessTree(snapshot, rootPid, {
    maxProcessCount = DEFAULT_LIMITS.maxProcessCount,
    maxDepth = DEFAULT_LIMITS.maxProcessDepth
} = {}) {
    positiveInteger(rootPid, 'root process id');
    positiveInteger(maxProcessCount, 'maximum process count');
    positiveInteger(maxDepth, 'maximum process depth');
    if (!Array.isArray(snapshot)) {
        throw new Error('process snapshot must be an array');
    }

    const rowsByPid = new Map();
    const childrenByParent = new Map();
    for (const row of snapshot) {
        if (row?.pid === 0) continue; // Windows exposes the System Idle Process as PID 0.
        const pid = positiveInteger(row?.pid, 'process snapshot pid');
        const parentPid = nonNegativeSafeInteger(row?.parentPid, 'process snapshot parent pid');
        const workingSetBytes = nonNegativeSafeInteger(
            row?.workingSetBytes,
            `process ${pid} working set`
        );
        if (rowsByPid.has(pid)) {
            throw new Error(`process snapshot contains duplicate pid ${pid}`);
        }
        const normalized = { pid, parentPid, workingSetBytes };
        rowsByPid.set(pid, normalized);
        const children = childrenByParent.get(parentPid) || [];
        children.push(normalized);
        childrenByParent.set(parentPid, children);
    }

    if (!rowsByPid.has(rootPid)) {
        throw new Error(`root process ${rootPid} was absent from the Windows process snapshot`);
    }

    const tree = [];
    const queue = [{ row: rowsByPid.get(rootPid), depth: 0 }];
    const visited = new Set();
    while (queue.length > 0) {
        const { row, depth } = queue.shift();
        if (visited.has(row.pid)) continue;
        visited.add(row.pid);

        if (tree.length >= maxProcessCount) {
            throw new Error(
                `application process tree exceeded the ${maxProcessCount}-process bound`
            );
        }
        tree.push(row);

        const children = (childrenByParent.get(row.pid) || [])
            .filter(child => !visited.has(child.pid));
        if (children.length > 0 && depth >= maxDepth) {
            throw new Error(
                `application process tree exceeded the ${maxDepth}-level depth bound`
            );
        }
        for (const child of children) queue.push({ row: child, depth: depth + 1 });
    }

    return tree;
}

async function queryProcessSnapshot({
    execFileImpl = childProcess.execFile,
    powershellCommand = DEFAULT_LIMITS.powershellCommand,
    timeoutMs = DEFAULT_LIMITS.processQueryTimeoutMs,
    maxBuffer = DEFAULT_LIMITS.processQueryMaxBuffer
} = {}) {
    positiveInteger(timeoutMs, 'process query timeout');
    const result = await execFileAsync(execFileImpl, powershellCommand, [
        '-NoLogo',
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        PROCESS_SNAPSHOT_SCRIPT
    ], {
        windowsHide: true,
        timeout: timeoutMs,
        maxBuffer
    });
    return parseProcessSnapshot(result.stdout);
}

async function sampleProcessTreeWorkingSet(rootPid, {
    querySnapshot = queryProcessSnapshot,
    maxProcessCount = DEFAULT_LIMITS.maxProcessCount,
    maxDepth = DEFAULT_LIMITS.maxProcessDepth,
    ...queryOptions
} = {}) {
    const snapshot = await querySnapshot(queryOptions);
    const tree = collectProcessTree(snapshot, rootPid, { maxProcessCount, maxDepth });
    const total = tree.reduce((sum, row) => sum + row.workingSetBytes, 0);
    if (!Number.isSafeInteger(total) || total <= 0) {
        throw new Error('application process tree working set was not a positive safe integer');
    }
    return total;
}

async function sampleWorkingSetWindow(rootPid, {
    windowMs = DEFAULT_PROTOCOL.memorySampleWindowMs,
    intervalMs = DEFAULT_PROTOCOL.memorySampleIntervalMs,
    now = () => performance.now(),
    sleep = defaultSleep,
    sampleProcessTree = sampleProcessTreeWorkingSet,
    ...sampleOptions
} = {}) {
    positiveInteger(windowMs, 'memory sample window');
    positiveInteger(intervalMs, 'memory sample interval');
    if (windowMs % intervalMs !== 0) {
        throw new Error('memory sample window must be divisible by the sample interval');
    }
    const sampleCount = windowMs / intervalMs;
    const values = [];
    let nextSampleAt = now();

    for (let index = 0; index < sampleCount; index += 1) {
        if (index > 0) {
            const delay = nextSampleAt - now();
            if (delay > 0) await sleep(delay);
        }
        const value = await sampleProcessTree(rootPid, sampleOptions);
        const bytes = typeof value === 'number'
            ? value
            : value?.workingSetBytes ?? value?.peakWorkingSetBytes;
        values.push(positiveFiniteNumber(bytes, `working-set sample ${index}`));
        nextSampleAt += intervalMs;
    }

    return {
        peakWorkingSetBytes: Math.max(...values),
        samples: values
    };
}

function createIsolatedProfile({
    temporaryDirectory = os.tmpdir(),
    prefix = 'deltamod-benchmark-profile-'
} = {}) {
    requireNonEmptyString(temporaryDirectory, 'temporary directory');
    requireNonEmptyString(prefix, 'profile prefix');
    fs.mkdirSync(temporaryDirectory, { recursive: true });
    const root = fs.mkdtempSync(path.join(temporaryDirectory, prefix));
    return path.resolve(root);
}

function removeIsolatedProfile(profilePath) {
    const resolved = path.resolve(requireNonEmptyString(profilePath, 'profile path'));
    if (!path.basename(resolved).startsWith('deltamod-benchmark-profile-')) {
        throw new Error(`refusing to remove a non-benchmark profile path: ${resolved}`);
    }
    fs.rmSync(resolved, { recursive: true, force: true, maxRetries: 3, retryDelay: 50 });
}

function copySeedDataRoot(sourceRoot, destinationRoot, {
    maxFiles = 10_000,
    maxBytes = 512 * 1024 * 1024
} = {}) {
    const source = path.resolve(requireNonEmptyString(sourceRoot, 'seed data root'));
    const destination = path.resolve(requireNonEmptyString(destinationRoot, 'benchmark data root'));
    const sourceMetadata = fs.lstatSync(source);
    if (!sourceMetadata.isDirectory() || sourceMetadata.isSymbolicLink()) {
        throw new Error('seed data root must be a regular directory');
    }
    fs.mkdirSync(destination, { recursive: true });
    let fileCount = 0;
    let totalBytes = 0;
    const pending = [[source, destination]];
    while (pending.length > 0) {
        const [currentSource, currentDestination] = pending.pop();
        for (const entry of fs.readdirSync(currentSource, { withFileTypes: true })) {
            const sourcePath = path.join(currentSource, entry.name);
            const destinationPath = path.join(currentDestination, entry.name);
            const metadata = fs.lstatSync(sourcePath);
            if (metadata.isSymbolicLink()) throw new Error('seed data root contains a link');
            if (metadata.isDirectory()) {
                fs.mkdirSync(destinationPath, { recursive: false });
                pending.push([sourcePath, destinationPath]);
                continue;
            }
            if (!metadata.isFile()) throw new Error('seed data root contains a non-regular entry');
            fileCount += 1;
            totalBytes += metadata.size;
            if (fileCount > maxFiles || totalBytes > maxBytes) {
                throw new Error('seed data root exceeded its bounded copy limit');
            }
            fs.copyFileSync(sourcePath, destinationPath, fs.constants.COPYFILE_EXCL);
        }
    }
    return { fileCount, totalBytes };
}

function buildLaunchEnvironment(profilePath, {
    executablePath,
    artifactPath,
    runIndex,
    measured,
    environment = process.env
}) {
    const root = path.resolve(requireNonEmptyString(profilePath, 'profile path'));
    const temporary = path.join(root, 'temp');
    const webviewProfile = path.join(root, 'webview2');
    fs.mkdirSync(temporary, { recursive: true });
    fs.mkdirSync(webviewProfile, { recursive: true });

    return {
        ...environment,
        TEMP: temporary,
        TMP: temporary,
        WEBVIEW2_USER_DATA_FOLDER: webviewProfile,
        DELTAMOD_BENCHMARK_PROFILE: root,
        DELTAMOD_BENCHMARK_PID: '',
        DELTAMOD_BENCHMARK_EXECUTABLE: executablePath,
        DELTAMOD_BENCHMARK_ARTIFACT: artifactPath,
        DELTAMOD_BENCHMARK_RUN: String(runIndex),
        DELTAMOD_BENCHMARK_MEASURED: measured ? 'true' : 'false'
    };
}

function inspectArtifact(artifactPath, executablePath, {
    maxFiles = DEFAULT_LIMITS.maxArtifactFiles,
    maxBytes = DEFAULT_LIMITS.maxArtifactBytes
} = {}) {
    const rootPath = path.resolve(requireNonEmptyString(artifactPath, 'artifact path'));
    const executable = path.resolve(requireNonEmptyString(executablePath, 'executable path'));
    positiveInteger(maxFiles, 'maximum artifact file count');
    positiveFiniteNumber(maxBytes, 'maximum artifact bytes');

    const rootStat = fs.lstatSync(rootPath);
    const executableStat = fs.statSync(executable);
    if (!executableStat.isFile()) {
        throw new Error(`executable path is not a regular file: ${executable}`);
    }

    let fileCount = 0;
    let totalBytes = 0;
    const pending = [rootPath];
    while (pending.length > 0) {
        const current = pending.pop();
        const stat = fs.lstatSync(current);
        if (stat.isSymbolicLink()) {
            throw new Error(`artifact contains a symbolic link: ${current}`);
        }
        if (stat.isDirectory()) {
            for (const entry of fs.readdirSync(current)) {
                pending.push(path.join(current, entry));
            }
            continue;
        }
        if (!stat.isFile()) {
            throw new Error(`artifact contains a non-regular entry: ${current}`);
        }
        fileCount += 1;
        totalBytes += stat.size;
        if (fileCount > maxFiles) {
            throw new Error(`artifact exceeded the ${maxFiles}-file bound`);
        }
        if (!Number.isSafeInteger(totalBytes) || totalBytes > maxBytes) {
            throw new Error(`artifact exceeded its ${maxBytes}-byte bound`);
        }
    }

    if (rootStat.isSymbolicLink()) {
        throw new Error(`artifact path cannot be a symbolic link: ${rootPath}`);
    }
    if (fileCount === 0 || totalBytes <= 0) {
        throw new Error(`artifact path contains no regular files: ${rootPath}`);
    }

    return {
        artifactPath: rootPath,
        path: rootPath,
        unpackedFileCount: fileCount,
        unpackedBytes: totalBytes,
        unpackedMiB: roundMiB(totalBytes),
        executablePath: executable,
        executableBytes: executableStat.size,
        executableMiB: roundMiB(executableStat.size)
    };
}

function readRegistryValue(stdout, valueName) {
    const expression = new RegExp(`\\s${valueName}\\s+REG_[A-Z0-9_]+\\s+(.+?)\\s*$`, 'mi');
    const match = String(stdout ?? '').match(expression);
    if (!match) throw new Error(`Windows registry value ${valueName} was not found`);
    return match[1].trim();
}

function normalizeWindowsProductName(productName, release) {
    const build = Number(String(release).split('.').at(-1));
    return Number.isInteger(build) && build >= 22_000
        ? productName.replace(/^Windows 10\b/, 'Windows 11')
        : productName;
}

function normalizeProcessorName(model) {
    return String(model)
        .replace(/\((?:R|TM)\)/g, '')
        .replace(/\s+CPU\s+@\s+.+$/i, '')
        .replace(/\s+/g, ' ')
        .trim();
}

async function commandVersion(execFileImpl, command, args = []) {
    const result = await execFileAsync(execFileImpl, command, args, {
        windowsHide: true,
        maxBuffer: 16 * 1024
    });
    const match = String(result.stdout).match(/\d+\.\d+(?:\.\d+)?(?:[-+][0-9A-Za-z.-]+)?/);
    if (!match) throw new Error(`unable to parse version from ${command}`);
    return match[0];
}

async function collectEnvironment({
    execFileImpl = childProcess.execFile,
    platform = process.platform,
    osModule = os
} = {}) {
    if (platform !== 'win32') {
        throw new Error('the desktop benchmark runner is Windows-only');
    }

    const registry = await execFileAsync(execFileImpl, 'reg.exe', [
        'query',
        'HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion',
        '/v',
        'ProductName'
    ], {
        windowsHide: true,
        maxBuffer: 16 * 1024
    });
    const cpus = osModule.cpus();
    if (!Array.isArray(cpus) || cpus.length === 0 || !cpus[0]?.model) {
        throw new Error('unable to identify the Windows processor');
    }
    const release = osModule.release();
    const productName = normalizeWindowsProductName(
        readRegistryValue(registry.stdout, 'ProductName'),
        release
    );

    return {
        operatingSystem: `${productName} ${release}`,
        processor: normalizeProcessorName(cpus[0].model),
        logicalProcessors: cpus.length,
        physicalMemoryBytes: nonNegativeSafeInteger(
            osModule.totalmem(),
            'physical memory bytes'
        ),
        node: process.version,
        npm: await commandVersion(execFileImpl, 'cmd.exe', ['/d', '/s', '/c', 'npm --version']),
        rustc: await commandVersion(execFileImpl, 'rustc.exe', ['--version']),
        cargo: await commandVersion(execFileImpl, 'cargo.exe', ['--version'])
    };
}

async function readGitRevision({
    cwd = process.cwd(),
    execFileImpl = childProcess.execFile
} = {}) {
    const result = await execFileAsync(execFileImpl, 'git.exe', ['rev-parse', 'HEAD'], {
        cwd,
        windowsHide: true,
        maxBuffer: 16 * 1024
    });
    return requireNonEmptyString(String(result.stdout).trim(), 'source revision');
}

function normalizePath(value, label, cwd) {
    return path.resolve(cwd, requireNonEmptyString(value, label));
}

function normalizeOptions(input = {}) {
    const cwd = path.resolve(input.cwd || process.cwd());
    const executablePath = normalizePath(
        input.executablePath ?? input.executable,
        'executable path',
        cwd
    );
    const artifactPath = normalizePath(
        input.artifactPath ?? input.artifact,
        'artifact path',
        cwd
    );
    const outputPath = normalizePath(
        input.outputPath ?? input.output,
        'output path',
        cwd
    );
    const args = input.args ?? [];
    if (!Array.isArray(args) || args.some(argument => typeof argument !== 'string')) {
        throw new Error('application args must be an array of strings');
    }

    const readinessProbe = input.readinessProbe;
    if (readinessProbe !== undefined && typeof readinessProbe !== 'function') {
        throw new Error('readinessProbe must be a function');
    }
    const hasReadinessCommand = input.readinessCommand !== undefined;
    const hasReadinessFile = input.readinessFile !== undefined;
    if (Number(hasReadinessCommand) + Number(hasReadinessFile) > 1 ||
        (readinessProbe && (hasReadinessCommand || hasReadinessFile))) {
        throw new Error('configure only one of readinessProbe, readinessCommand, or readinessFile');
    }
    if (!readinessProbe && !hasReadinessCommand && !hasReadinessFile) {
        throw new Error(
            'readiness command, readiness file, or injected readinessProbe is required; readiness is never inferred'
        );
    }

    const sourceRevision = input.sourceRevision;
    if (sourceRevision !== undefined) requireNonEmptyString(sourceRevision, 'source revision');
    if (input.includesRevision !== undefined && input.includesRevision !== null) {
        requireNonEmptyString(input.includesRevision, 'includes revision');
    }

    const seedDataRoot = input.seedDataRoot === undefined
        ? null
        : normalizePath(input.seedDataRoot, 'seed data root', cwd);
    return {
        ...input,
        cwd,
        executablePath,
        artifactPath,
        outputPath,
        args: [...args],
        seedDataRoot,
        readinessProbe,
        readinessCommand: input.readinessCommand === undefined
            ? undefined
            : parseCommandConfig(input.readinessCommand),
        readinessFile: input.readinessFile === undefined
            ? undefined
            : requireNonEmptyString(input.readinessFile, 'readiness file'),
        sourceRevision,
        includesRevision: input.includesRevision ?? null,
        readinessTimeoutMs: input.readinessTimeoutMs ?? DEFAULT_LIMITS.readinessTimeoutMs,
        readinessPollIntervalMs: input.readinessPollIntervalMs ?? DEFAULT_LIMITS.readinessPollIntervalMs,
        readinessCommandTimeoutMs: input.readinessCommandTimeoutMs
            ?? DEFAULT_LIMITS.readinessCommandTimeoutMs,
        powershellCommand: input.powershellCommand ?? DEFAULT_LIMITS.powershellCommand
    };
}

function validateInputPaths(options) {
    const executableStat = fs.statSync(options.executablePath);
    if (!executableStat.isFile()) {
        throw new Error(`executable path is not a regular file: ${options.executablePath}`);
    }
    fs.lstatSync(options.artifactPath);
    if (options.seedDataRoot) {
        const seed = fs.lstatSync(options.seedDataRoot);
        if (!seed.isDirectory() || seed.isSymbolicLink()) {
            throw new Error(`seed data root is not a regular directory: ${options.seedDataRoot}`);
        }
    }
    if (fs.existsSync(options.outputPath)) {
        throw new Error(
            `refusing to overwrite existing immutable benchmark result: ${options.outputPath}`
        );
    }
    fs.mkdirSync(path.dirname(options.outputPath), { recursive: true });
}

function writeImmutableJson(outputPath, value) {
    const target = path.resolve(requireNonEmptyString(outputPath, 'output path'));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    if (fs.existsSync(target)) {
        throw new Error(`refusing to overwrite existing immutable benchmark result: ${target}`);
    }

    const temporary = path.join(
        path.dirname(target),
        `.${path.basename(target)}.${process.pid}.${Date.now()}.${Math.random()
            .toString(16).slice(2)}.tmp`
    );
    let descriptor;
    try {
        descriptor = fs.openSync(temporary, 'wx');
        const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`, 'utf8');
        fs.writeFileSync(descriptor, bytes);
        fs.fsyncSync(descriptor);
        fs.closeSync(descriptor);
        descriptor = undefined;

        // A hard link publishes the complete file without permitting an existing
        // result to be replaced. The temporary and target are kept in one directory.
        fs.linkSync(temporary, target);
        fs.unlinkSync(temporary);
    } catch (error) {
        if (descriptor !== undefined) {
            try { fs.closeSync(descriptor); } catch { /* preserve the write error */ }
        }
        try { fs.unlinkSync(temporary); } catch { /* the file may not have been created */ }
        throw error;
    }
}

function isProcessNotFoundError(error) {
    if (error?.code === 128) return true;
    const text = `${error?.stdout || ''}\n${error?.stderr || ''}`.toLowerCase();
    return text.includes('no running instance') || text.includes('not found');
}

async function killProcessTree(pid, {
    execFileImpl = childProcess.execFile,
    platform = process.platform,
    taskkillCommand = 'taskkill.exe'
} = {}) {
    positiveInteger(pid, 'root process id');
    if (platform !== 'win32') {
        try {
            process.kill(pid, 'SIGKILL');
        } catch (error) {
            if (error?.code !== 'ESRCH') throw error;
        }
        return;
    }

    try {
        await execFileAsync(execFileImpl, taskkillCommand, [
            '/PID', String(pid), '/T', '/F'
        ], {
            windowsHide: true,
            maxBuffer: 16 * 1024
        });
    } catch (error) {
        if (!isProcessNotFoundError(error)) {
            throw new Error(`failed to kill application process tree ${pid}: ${error.message}`);
        }
    }
}

function observeChildError(child) {
    let error;
    const onError = value => { error = value; };
    if (typeof child?.on === 'function') child.on('error', onError);
    return {
        get error() { return error; },
        detach() {
            if (typeof child?.off === 'function') child.off('error', onError);
            else if (typeof child?.removeListener === 'function') child.removeListener('error', onError);
        }
    };
}

function attachCleanupError(primaryError, cleanupError) {
    if (!cleanupError) return primaryError;
    if (!primaryError) return cleanupError;
    primaryError.cleanupError = cleanupError;
    return primaryError;
}

async function runLaunch({
    options,
    runIndex,
    measured,
    probe,
    dependencies,
    usedProfiles
}) {
    let profilePath;
    let child;
    let result;
    let primaryError;
    let ownsProfile = false;
    let dataRoot;
    let readyFile;
    let stdout = '';
    let stderr = '';

    try {
        profilePath = await dependencies.createProfile({ runIndex, measured });
        profilePath = path.resolve(requireNonEmptyString(profilePath, 'created profile path'));
        if (usedProfiles.has(profilePath)) {
            throw new Error(`profile factory reused an application profile: ${profilePath}`);
        }
        usedProfiles.add(profilePath);
        ownsProfile = true;

        const environment = buildLaunchEnvironment(profilePath, {
            executablePath: options.executablePath,
            artifactPath: options.artifactPath,
            runIndex,
            measured,
            environment: options.environment || process.env
        });
        dataRoot = path.join(profilePath, 'data-root');
        fs.mkdirSync(dataRoot, { recursive: true });
        if (options.seedDataRoot) copySeedDataRoot(options.seedDataRoot, dataRoot);
        readyFile = path.join(dataRoot, '.deltamod-benchmark-ready');
        environment.DELTAMOD_SMOKE_DATA_ROOT = dataRoot;
        environment.DELTAMOD_BENCHMARK_READY_FILE = readyFile;
        const launchedAt = dependencies.now();
        child = dependencies.spawn(options.executablePath, [
            ...options.args,
            '--data-root',
            dataRoot
        ], {
            cwd: path.dirname(options.executablePath),
            env: environment,
            windowsHide: true,
            stdio: ['ignore', 'pipe', 'pipe']
        });
        const appendDiagnostic = (current, chunk) => (
            `${current}${Buffer.from(chunk).toString('utf8')}`.slice(-64 * 1024)
        );
        child.stdout?.on('data', chunk => { stdout = appendDiagnostic(stdout, chunk); });
        child.stderr?.on('data', chunk => { stderr = appendDiagnostic(stderr, chunk); });
        if (!child || !positiveInteger(child.pid, 'spawned application process id')) {
            throw new Error('application did not provide a valid process id after launch');
        }

        const childError = observeChildError(child);
        try {
            environment.DELTAMOD_BENCHMARK_PID = String(child.pid);
            const readinessContext = {
                pid: child.pid,
                profilePath,
                executablePath: options.executablePath,
                artifactPath: options.artifactPath,
                runIndex,
                measured,
                readiness: DEFAULT_PROTOCOL.readiness,
                readyFile,
                environment
            };
            const readyAt = await waitForReadiness({
                child,
                probe,
                context: readinessContext,
                timeoutMs: options.readinessTimeoutMs,
                pollIntervalMs: options.readinessPollIntervalMs,
                now: dependencies.now,
                sleep: dependencies.sleep,
                isAlive: childValue => !childError.error && dependencies.isChildAlive(childValue)
            });
            if (childError.error) throw childError.error;

            if (measured) {
                const memory = await dependencies.sampleWindow(child.pid, {
                    windowMs: DEFAULT_PROTOCOL.memorySampleWindowMs,
                    intervalMs: DEFAULT_PROTOCOL.memorySampleIntervalMs,
                    now: dependencies.now,
                    sleep: dependencies.sleep,
                    execFileImpl: dependencies.execFile,
                    powershellCommand: options.powershellCommand,
                    maxProcessCount: DEFAULT_LIMITS.maxProcessCount,
                    maxDepth: DEFAULT_LIMITS.maxProcessDepth
                });
                result = {
                    readyMs: positiveFiniteNumber(readyAt - launchedAt, 'ready time'),
                    peakWorkingSetBytes: positiveFiniteNumber(
                        typeof memory === 'number'
                            ? memory
                            : memory?.peakWorkingSetBytes,
                        'peak working set'
                    )
                };
            }
        } finally {
            childError.detach();
        }
    } catch (error) {
        const diagnostics = [stderr.trim(), stdout.trim()].filter(Boolean).join('\n');
        const markerState = readyFile ? fs.existsSync(readyFile) : false;
        primaryError = new Error(
            `benchmark launch ${runIndex} failed (marker=${markerState}, profile=${profilePath || '<none>'}): ${error?.message || String(error)}`
            + (diagnostics ? `\n${diagnostics}` : '')
        );
    }

    let cleanupError;
    if (child?.pid) {
        try {
            await dependencies.killTree(child.pid);
        } catch (error) {
            cleanupError = error;
        }
    }
    if (profilePath && ownsProfile) {
        try {
            await dependencies.removeProfile(profilePath);
        } catch (error) {
            cleanupError = cleanupError || error;
        }
    }

    const error = attachCleanupError(primaryError, cleanupError);
    if (error) throw error;
    if (!result && measured) throw new Error(`measured launch ${runIndex} produced no result`);
    return result;
}

function defaultSleep(milliseconds) {
    return new Promise(resolve => setTimeout(resolve, milliseconds));
}

async function runBenchmark(inputOptions, injectedDependencies = {}) {
    const options = normalizeOptions(inputOptions);
    validateInputPaths(options);

    const dependencies = {
        spawn: childProcess.spawn,
        execFile: childProcess.execFile,
        now: () => performance.now(),
        sleep: defaultSleep,
        isChildAlive: defaultChildIsAlive,
        createProfile: createIsolatedProfile,
        removeProfile: removeIsolatedProfile,
        inspectArtifact: (artifactPath, executablePath) => inspectArtifact(
            artifactPath,
            executablePath
        ),
        collectEnvironment: ({ execFileImpl }) => collectEnvironment({
            execFileImpl,
            platform: process.platform
        }),
        getSourceRevision: ({ cwd, execFileImpl }) => readGitRevision({
            cwd,
            execFileImpl
        }),
        sampleWindow: (pid, sampleOptions) => sampleWorkingSetWindow(pid, {
            ...sampleOptions,
            sampleProcessTree: sampleProcessTreeWorkingSet
        }),
        writeResult: writeImmutableJson,
        ...injectedDependencies
    };
    if (!injectedDependencies.killTree) {
        dependencies.killTree = pid => killProcessTree(pid, {
            execFileImpl: dependencies.execFile,
            platform: process.platform
        });
    }

    const sourceRevision = options.sourceRevision || await dependencies.getSourceRevision({
        cwd: options.cwd,
        execFileImpl: dependencies.execFile
    });
    requireNonEmptyString(sourceRevision, 'source revision');

    const environment = options.environment || await dependencies.collectEnvironment({
        execFileImpl: dependencies.execFile
    });
    if (!environment || typeof environment !== 'object') {
        throw new Error('benchmark environment metadata is required');
    }

    const artifact = await dependencies.inspectArtifact(
        options.artifactPath,
        options.executablePath
    );
    if (!artifact || !Number.isFinite(artifact.unpackedBytes) || artifact.unpackedBytes <= 0) {
        throw new Error('artifact inspection did not return a positive unpacked byte count');
    }

    const probe = options.readinessProbe || (
        options.readinessCommand
            ? createReadinessProbe(options.readinessCommand, {
                execFileImpl: dependencies.execFile,
                commandTimeoutMs: options.readinessCommandTimeoutMs
            })
            : createReadinessFileProbe(options.readinessFile, { cwd: options.cwd })
    );
    const samples = [];
    const usedProfiles = new Set();
    const totalLaunches = DEFAULT_PROTOCOL.warmupLaunches + DEFAULT_PROTOCOL.measuredLaunches;

    for (let launchIndex = 0; launchIndex < totalLaunches; launchIndex += 1) {
        const measured = launchIndex >= DEFAULT_PROTOCOL.warmupLaunches;
        const sample = await runLaunch({
            options,
            runIndex: launchIndex,
            measured,
            probe,
            dependencies,
            usedProfiles
        });
        if (measured) samples.push(sample);
    }

    const result = {
        schemaVersion: 1,
        runtime: 'tauri',
        sourceRevision,
        includesRevision: options.includesRevision,
        capturedAt: options.capturedAt || new Date().toISOString().slice(0, 10),
        environment,
        protocol: { ...DEFAULT_PROTOCOL },
        samples,
        summary: summarizeSamples(samples),
        artifact: {
            ...artifact,
            path: artifact.path || artifact.artifactPath,
            artifactPath: artifact.artifactPath || artifact.path
        },
        postRewriteComparison: null
    };

    await dependencies.writeResult(options.outputPath, result);
    return result;
}

function parseArguments(argv) {
    const result = { args: [], readinessArgs: [] };
    const getValue = (index, name, allowOptionLike = false) => {
        const value = argv[index + 1];
        if (value === undefined || (!allowOptionLike && value.startsWith('--'))) {
            throw new Error(`${name} requires a value`);
        }
        return value;
    };

    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index];
        if (argument === '--help' || argument === '-h') {
            result.help = true;
            continue;
        }
        if (argument === '--executable' || argument === '--executable-path') {
            result.executablePath = getValue(index, argument);
            index += 1;
        } else if (argument === '--artifact' || argument === '--artifact-path') {
            result.artifactPath = getValue(index, argument);
            index += 1;
        } else if (argument === '--output' || argument === '--output-path') {
            result.outputPath = getValue(index, argument);
            index += 1;
        } else if (argument === '--seed-data-root') {
            result.seedDataRoot = getValue(index, argument);
            index += 1;
        } else if (argument === '--arg' || argument === '--app-arg') {
            result.args.push(getValue(index, argument, true));
            index += 1;
        } else if (argument === '--readiness-command' || argument === '--readiness') {
            result.readinessCommand = getValue(index, argument);
            index += 1;
        } else if (argument === '--readiness-file') {
            result.readinessFile = getValue(index, argument);
            index += 1;
        } else if (argument === '--readiness-arg') {
            result.readinessArgs.push(getValue(index, argument, true));
            index += 1;
        } else if (argument === '--readiness-args') {
            const encoded = getValue(index, argument);
            let decoded;
            try { decoded = JSON.parse(encoded); } catch (error) {
                throw new Error(`--readiness-args must be JSON: ${error.message}`);
            }
            if (!Array.isArray(decoded) || decoded.some(value => typeof value !== 'string')) {
                throw new Error('--readiness-args must be a JSON array of strings');
            }
            result.readinessArgs.push(...decoded);
            index += 1;
        } else if (argument === '--readiness-timeout-ms') {
            result.readinessTimeoutMs = Number(getValue(index, argument));
            index += 1;
        } else if (argument === '--readiness-poll-ms') {
            result.readinessPollIntervalMs = Number(getValue(index, argument));
            index += 1;
        } else if (argument === '--readiness-command-timeout-ms') {
            result.readinessCommandTimeoutMs = Number(getValue(index, argument));
            index += 1;
        } else if (argument === '--source-revision') {
            result.sourceRevision = getValue(index, argument);
            index += 1;
        } else if (argument === '--includes-revision') {
            result.includesRevision = getValue(index, argument);
            index += 1;
        } else if (argument === '--cwd') {
            result.cwd = getValue(index, argument);
            index += 1;
        } else if (argument === '--powershell-command') {
            result.powershellCommand = getValue(index, argument);
            index += 1;
        } else {
            throw new Error(`unknown argument: ${argument}`);
        }
    }

    if (result.help) return result;
    if (!result.executablePath) throw new Error('--executable is required');
    if (!result.artifactPath) throw new Error('--artifact is required');
    if (!result.outputPath) throw new Error('--output is required');
    if (!result.readinessCommand && !result.readinessFile) {
        throw new Error(
            'provide --readiness-command or --readiness-file; readiness must be observable'
        );
    }
    if (result.readinessCommand) {
        result.readinessCommand = {
            command: result.readinessCommand,
            args: result.readinessArgs
        };
    }
    delete result.readinessArgs;
    return result;
}

function usage() {
    return [
        'Usage: node scripts/desktop-benchmark/capture-windows.js',
        '  --executable <path> --artifact <path> --output <path>',
        '  [--seed-data-root <path>]',
        '  (--readiness-command <executable> [--readiness-arg <arg>]... |',
        '   --readiness-file <path>)',
        '  [--arg <arg>]... [--source-revision <revision>]',
        '',
        'The readiness command is polled until it exits successfully. It receives',
        'DELTAMOD_BENCHMARK_PID, DELTAMOD_BENCHMARK_PROFILE, and the other benchmark',
        'context values in its environment. Argument placeholders include {pid},',
        '{profile}, {executable}, {artifact}, {run}, {measured}, {readiness},',
        'and {readyFile}.',
        'There is no implicit readiness fallback.'
    ].join('\n');
}

async function main(argv = process.argv.slice(2)) {
    const options = parseArguments(argv);
    if (options.help) {
        console.log(usage());
        return null;
    }
    if (process.platform !== 'win32') {
        throw new Error('capture-windows.js must run on Windows');
    }
    const result = await runBenchmark(options);
    console.log(`Wrote immutable benchmark result to ${options.outputPath}`);
    return result;
}

if (require.main === module) {
    main().catch(error => {
        console.error(error instanceof Error ? error.message : String(error));
        process.exitCode = 1;
    });
}

module.exports = {
    DEFAULT_LIMITS,
    DEFAULT_PROTOCOL,
    buildLaunchEnvironment,
    collectProcessTree,
    collectEnvironment,
    createIsolatedProfile,
    copySeedDataRoot,
    createReadinessFileProbe,
    createReadinessProbe,
    inspectArtifact,
    killProcessTree,
    main,
    median,
    nearestRank,
    normalizeProcessorName,
    normalizeOptions,
    normalizeWindowsProductName,
    parseArguments,
    parseProcessSnapshot,
    parseReadinessOutput,
    queryProcessSnapshot,
    readGitRevision,
    removeIsolatedProfile,
    runBenchmark,
    runLaunch,
    sampleProcessTreeWorkingSet,
    sampleWorkingSetWindow,
    summarizeSamples,
    usage,
    waitForReadiness,
    writeImmutableJson
};
