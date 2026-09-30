// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const childProcess = require('node:child_process');
const { performance } = require('node:perf_hooks');

function launchOrder(measuredLaunches = 7) {
    const order = [
        { label: 'baseline', runIndex: 0, measured: false },
        { label: 'candidate', runIndex: 0, measured: false }
    ];
    for (let round = 1; round <= measuredLaunches; round += 1) {
        for (const label of round % 2 ? ['baseline', 'candidate'] : ['candidate', 'baseline']) {
            order.push({ label, runIndex: round, measured: true });
        }
    }
    return order;
}

function validateConfig(config) {
    if (!config || typeof config !== 'object') throw new Error('Paired benchmark configuration required');
    for (const label of ['baseline', 'candidate']) {
        const item = config[label];
        if (!item || !/^[0-9a-f]{40}$/i.test(item.sourceRevision || '')) throw new Error(`${label}: exact revision required`);
        for (const field of ['executablePath', 'artifactPath']) {
            if (typeof item[field] !== 'string' || !path.isAbsolute(item[field])) throw new Error(`${label}: absolute ${field} required`);
        }
        const root = fs.realpathSync(item.artifactPath);
        const executable = fs.realpathSync(item.executablePath);
        const relative = path.relative(root, executable);
        if (!fs.statSync(root).isDirectory() || !fs.statSync(executable).isFile() || !relative || relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
            throw new Error(`${label}: executable must be inside its complete installed directory`);
        }
    }
    if (fs.realpathSync(config.baseline.artifactPath) === fs.realpathSync(config.candidate.artifactPath)) {
        throw new Error('Baseline and candidate installations must be distinct');
    }
    if (!path.isAbsolute(config.seedDataRoot || '') || !fs.statSync(config.seedDataRoot).isDirectory()) throw new Error('An explicit common seed fixture is required');
    if (!path.isAbsolute(config.outputDirectory || '')) throw new Error('An absolute output directory is required');
    return config;
}

function traceCollector() {
    let buffer = '';
    const records = [];
    return {
        records,
        accept(chunk) {
            buffer = (buffer + Buffer.from(chunk).toString('utf8')).slice(-8192);
            const lines = buffer.split(/\r?\n/);
            buffer = lines.pop();
            for (const line of lines) {
                const index = line.indexOf('DELTAMOD_STARTUP ');
                if (index < 0 || records.length >= 4) continue;
                try {
                    const value = JSON.parse(line.slice(index + 'DELTAMOD_STARTUP '.length));
                    if (value && ['renderer-ready', 'boot-dismissed'].includes(value.phase)) records.push(value);
                } catch { /* Incomplete diagnostic text is not a readiness signal. */ }
            }
        }
    };
}

async function runPair(config, injected = {}) {
    validateConfig(config);
    const capture = injected.capture || require('./capture-windows');
    const compare = injected.compare || require('./compare').compareBenchmarkResults;
    const clock = injected.now || (() => performance.now());
    fs.mkdirSync(config.outputDirectory, { recursive: true });
    const output = name => path.join(config.outputDirectory, name);
    for (const file of ['baseline.json', 'candidate.json', 'comparison.json', 'pair-diagnostics.json', 'pair-error.json']) {
        if (fs.existsSync(output(file))) throw new Error('Refusing to overwrite a previous paired benchmark');
    }
    const environment = await capture.collectEnvironment();
    const usedProfiles = new Set();
    const samples = { baseline: [], candidate: [] };
    const diagnostics = [];
    const options = Object.fromEntries(['baseline', 'candidate'].map(label => [label, capture.normalizeOptions({
        ...config[label],
        outputPath: output(`${label}.json`),
        seedDataRoot: config.seedDataRoot,
        readinessFile: '{readyFile}',
        includesRevision: config[label].sourceRevision
    })]));
    const probe = capture.createReadinessFileProbe('{readyFile}');
    const order = launchOrder(capture.DEFAULT_PROTOCOL.measuredLaunches);
    try {
        for (const launch of order) {
            const trace = traceCollector();
            const observations = [];
            let windowElapsedMs = null;
            const dependencies = {
                now: clock,
                sleep: ms => new Promise(resolve => setTimeout(resolve, ms)),
                createProfile: () => capture.createIsolatedProfile(),
                removeProfile: capture.removeIsolatedProfile,
                killTree: capture.killProcessTree,
                isChildAlive: child => child.exitCode == null && child.signalCode == null,
                execFile: childProcess.execFile,
                spawn(...args) {
                    const child = childProcess.spawn(...args);
                    child.stdout?.on('data', trace.accept);
                    child.stderr?.on('data', trace.accept);
                    return child;
                },
                async sampleWindow(pid, sampling) {
                    const start = clock();
                    const result = await capture.sampleWorkingSetWindow(pid, {
                        ...sampling,
                        async sampleProcessTree(processId, queryOptions) {
                            const sampledAt = clock();
                            const bytes = await capture.sampleProcessTreeWorkingSet(processId, queryOptions);
                            observations.push({ offsetMs: sampledAt - start, queryMs: clock() - sampledAt, bytes });
                            return bytes;
                        }
                    });
                    windowElapsedMs = clock() - start;
                    return result;
                }
            };
            const sample = await capture.runLaunch({
                options: options[launch.label], ...launch, probe, dependencies, usedProfiles
            });
            diagnostics.push({ ...launch, windowElapsedMs, observations, traces: trace.records });
            if (launch.measured) samples[launch.label].push(sample);
            console.log(`${launch.label} ${launch.measured ? `sample ${launch.runIndex}` : 'warmup'} complete`);
        }
    } catch (error) {
        capture.writeImmutableJson(output('pair-error.json'), {
            schemaVersion: 1, error: String(error.message || error), completed: diagnostics,
            note: 'Incomplete run: no successful comparison is reported.'
        });
        throw error;
    }
    const records = {};
    for (const label of ['baseline', 'candidate']) {
        records[label] = {
            schemaVersion: 1, runtime: 'tauri',
            sourceRevision: config[label].sourceRevision,
            includesRevision: config[label].sourceRevision,
            capturedAt: new Date().toISOString(),
            environment,
            protocol: { ...capture.DEFAULT_PROTOCOL },
            samples: samples[label],
            summary: capture.summarizeSamples(samples[label]),
            artifact: capture.inspectArtifact(config[label].artifactPath, config[label].executablePath),
            postRewriteComparison: null
        };
        capture.writeImmutableJson(output(`${label}.json`), records[label]);
    }
    const comparison = compare(records.baseline, records.candidate);
    capture.writeImmutableJson(output('comparison.json'), comparison);
    capture.writeImmutableJson(output('pair-diagnostics.json'), {
        schemaVersion: 1, environment, order, launches: diagnostics,
        notes: [
            'Both packages run on this same host with alternating measured launch order and the same fixture.',
            'All seven measurements are retained. Nearest-rank p95 with seven samples is the maximum.',
            'The legacy sampler is shared by both builds. Query overhead can stretch its nominal 2-second window; actual observation timing is recorded.',
            'Renderer IPC traces are diagnostic only and are emitted after the authenticated readiness marker.',
            'Installer download bytes are recorded by the separate package footprint audits, not as unpacked bytes.'
        ]
    });
    return comparison;
}

if (require.main === module) {
    if (process.platform !== 'win32') throw new Error('The paired packaged benchmark requires Windows');
    const configPath = process.argv[2];
    if (!configPath || fs.statSync(configPath).size > 65536) throw new Error('Usage: node paired-windows.cjs <bounded-config.json>');
    runPair(JSON.parse(fs.readFileSync(configPath, 'utf8').replace(/^\uFEFF/, '')))
        .catch(error => { console.error(error.message); process.exitCode = 1; });
}
module.exports = { launchOrder, validateConfig, traceCollector, runPair };
