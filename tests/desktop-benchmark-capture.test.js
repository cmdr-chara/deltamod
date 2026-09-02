// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const {
    DEFAULT_PROTOCOL,
    collectProcessTree,
    createReadinessFileProbe,
    createReadinessProbe,
    inspectArtifact,
    parseArguments,
    parseProcessSnapshot,
    runBenchmark,
    sampleWorkingSetWindow,
    summarizeSamples,
    waitForReadiness,
    writeImmutableJson
} = require('../scripts/desktop-benchmark/capture-windows');

const temporaryDirectories = [];

afterEach(() => {
    while (temporaryDirectories.length) {
        fs.rmSync(temporaryDirectories.pop(), { recursive: true, force: true });
    }
});

function temporaryDirectory(prefix) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
    temporaryDirectories.push(directory);
    return directory;
}

function fakeChild(pid) {
    const child = new EventEmitter();
    child.pid = pid;
    child.exitCode = null;
    child.signalCode = null;
    return child;
}

describe('Windows desktop benchmark capture runner', () => {
    test('requires explicit paths and an observable readiness configuration', () => {
        expect(() => parseArguments([
            '--artifact', 'artifact',
            '--output', 'result.json'
        ])).toThrow('--executable is required');

        expect(() => parseArguments([
            '--executable', 'candidate.exe',
            '--artifact', 'artifact',
            '--output', 'result.json'
        ])).toThrow(/readiness.*observable/);

        const options = parseArguments([
            '--executable', 'candidate.exe',
            '--artifact', 'artifact',
            '--output', 'result.json',
            '--readiness-command', 'node.exe',
            '--readiness-arg', '{pid}',
            '--arg', '--benchmark-mode'
        ]);
        expect(options).toMatchObject({
            executablePath: 'candidate.exe',
            artifactPath: 'artifact',
            outputPath: 'result.json',
            args: ['--benchmark-mode'],
            readinessCommand: {
                command: 'node.exe',
                args: ['{pid}']
            }
        });
    });

    test('parses and bounds a process tree before summing working sets', () => {
        const snapshot = parseProcessSnapshot(JSON.stringify([
            { ProcessId: '100', ParentProcessId: '1', WorkingSetSize: '10' },
            { ProcessId: '101', ParentProcessId: '100', WorkingSetSize: '20' },
            { ProcessId: '102', ParentProcessId: '101', WorkingSetSize: '30' },
            { ProcessId: '999', ParentProcessId: '1', WorkingSetSize: '9000' }
        ]));

        expect(collectProcessTree(snapshot, 100)).toEqual([
            { pid: 100, parentPid: 1, workingSetBytes: 10 },
            { pid: 101, parentPid: 100, workingSetBytes: 20 },
            { pid: 102, parentPid: 101, workingSetBytes: 30 }
        ]);
        expect(() => collectProcessTree(snapshot, 100, { maxProcessCount: 2 }))
            .toThrow(/2-process bound/);
        expect(() => collectProcessTree(snapshot, 100, { maxDepth: 1 }))
            .toThrow(/1-level depth bound/);
    });

    test('samples a bounded 2-second window at the fixed 100 ms cadence', async () => {
        const values = [];
        let currentTime = 0;
        const result = await sampleWorkingSetWindow(100, {
            now: () => currentTime,
            sleep: async milliseconds => { currentTime += milliseconds; },
            sampleProcessTree: async (_pid, options) => {
                values.push({ pid: _pid, options });
                return values.length * 10;
            }
        });

        expect(values).toHaveLength(DEFAULT_PROTOCOL.memorySampleWindowMs
            / DEFAULT_PROTOCOL.memorySampleIntervalMs);
        expect(result.peakWorkingSetBytes).toBe(values.length * 10);
        expect(currentTime).toBe(1900);
    });

    test('waits for the injected readiness probe and fails closed on timeout', async () => {
        const child = fakeChild(200);
        let polls = 0;
        let now = 0;
        await expect(waitForReadiness({
            child,
            context: { pid: child.pid },
            probe: async () => ++polls >= 3,
            timeoutMs: 100,
            pollIntervalMs: 10,
            now: () => now,
            sleep: async milliseconds => { now += milliseconds; }
        })).resolves.toBe(20);
        expect(polls).toBe(3);

        now = 0;
        await expect(waitForReadiness({
            child,
            context: { pid: child.pid },
            probe: async () => false,
            timeoutMs: 20,
            pollIntervalMs: 10,
            now: () => now,
            sleep: async milliseconds => { now += milliseconds; }
        })).rejects.toThrow(/readiness was not observed/);
    });

    test('command readiness forwards the isolated process context without inventing a signal', async () => {
        const calls = [];
        const probe = createReadinessProbe({
            command: 'readiness-fixture.exe',
            args: ['--pid', '{pid}', '--profile', '{profile}']
        }, {
            execFileImpl: (command, args, options, callback) => {
                calls.push({ command, args, options });
                callback(null, '{"ready":true}', '');
            }
        });

        await expect(probe({
            pid: 321,
            profilePath: 'C:\\temp\\profile',
            executablePath: 'C:\\app\\candidate.exe',
            artifactPath: 'C:\\app',
            runIndex: 4,
            readiness: DEFAULT_PROTOCOL.readiness,
            environment: { TEST_BENCHMARK: '1' }
        })).resolves.toBe(true);
        expect(calls[0]).toMatchObject({
            command: 'readiness-fixture.exe',
            args: ['--pid', '321', '--profile', 'C:\\temp\\profile']
        });
        expect(calls[0].options.env).toMatchObject({
            TEST_BENCHMARK: '1',
            DELTAMOD_BENCHMARK_PID: '321',
            DELTAMOD_BENCHMARK_READINESS: DEFAULT_PROTOCOL.readiness
        });
    });

    test('supports an explicitly configured readiness marker file', async () => {
        const root = temporaryDirectory('deltamod-readiness-file-');
        const marker = path.join(root, 'ready');
        const probe = createReadinessFileProbe('{readyFile}', { cwd: root });
        const context = { pid: 123, profilePath: root, readyFile: marker };

        await expect(probe(context)).resolves.toBe(false);
        fs.writeFileSync(marker, 'ready');
        await expect(probe(context)).resolves.toBe(true);
    });

    test('runs one warmup and seven measured launches with fresh profiles', async () => {
        const root = temporaryDirectory('deltamod-capture-test-');
        const artifact = path.join(root, 'artifact');
        const executable = path.join(artifact, 'Deltamod Community.exe');
        const output = path.join(root, 'result.json');
        fs.mkdirSync(artifact);
        fs.writeFileSync(executable, 'fixture executable');
        fs.writeFileSync(path.join(artifact, 'resource.bin'), 'fixture resource');

        const launches = [];
        const killed = [];
        let nextPid = 10_000;
        let currentTime = 0;
        const environment = {
            operatingSystem: 'Windows 11 Pro 10.0.26200',
            processor: 'fixture CPU',
            logicalProcessors: 16,
            physicalMemoryBytes: 16 * 1024 * 1024 * 1024,
            node: 'v24.19.0',
            npm: '11.17.0',
            rustc: '1.97.1',
            cargo: '1.97.1'
        };

        const result = await runBenchmark({
            executablePath: executable,
            artifactPath: artifact,
            outputPath: output,
            sourceRevision: 'f'.repeat(40),
            includesRevision: 'fixture',
            capturedAt: '2026-08-31',
            readinessProbe: async context => {
                launches.push({ phase: 'readiness', context });
                return true;
            }
        }, {
            now: () => {
                currentTime += 1;
                return currentTime;
            },
            spawn: (command, args, options) => {
                const child = fakeChild(++nextPid);
                launches.push({ phase: 'spawn', command, args, options, child });
                return child;
            },
            isChildAlive: () => true,
            createProfile: ({ runIndex }) => {
                const profile = path.join(root, `deltamod-benchmark-profile-${runIndex}-`);
                fs.mkdirSync(profile);
                return profile;
            },
            removeProfile: profile => fs.rmSync(profile, { recursive: true, force: true }),
            killTree: pid => { killed.push(pid); },
            sampleWindow: async pid => ({ peakWorkingSetBytes: pid }),
            collectEnvironment: async () => environment,
            inspectArtifact,
            getSourceRevision: async () => 'unused'
        });

        expect(result.protocol).toEqual(DEFAULT_PROTOCOL);
        expect(result.samples).toHaveLength(7);
        expect(launches.filter(entry => entry.phase === 'spawn')).toHaveLength(8);
        expect(launches.filter(entry => entry.phase === 'readiness')).toHaveLength(8);
        expect(new Set(launches
            .filter(entry => entry.phase === 'readiness')
            .map(entry => entry.context.profilePath)).size).toBe(8);
        expect(killed).toHaveLength(8);
        expect(fs.existsSync(output)).toBe(true);
        expect(JSON.parse(fs.readFileSync(output, 'utf8'))).toEqual(result);
        expect(summarizeSamples(result.samples)).toEqual(result.summary);
    });

    test('does not publish a raw result when readiness never arrives', async () => {
        const root = temporaryDirectory('deltamod-capture-fail-');
        const artifact = path.join(root, 'artifact');
        const executable = path.join(artifact, 'candidate.exe');
        const output = path.join(root, 'result.json');
        fs.mkdirSync(artifact);
        fs.writeFileSync(executable, 'fixture executable');
        let currentTime = 0;

        await expect(runBenchmark({
            executablePath: executable,
            artifactPath: artifact,
            outputPath: output,
            sourceRevision: 'f'.repeat(40),
            readinessProbe: async () => false,
            readinessTimeoutMs: 1,
            readinessPollIntervalMs: 1
        }, {
            now: () => currentTime,
            spawn: () => fakeChild(20_001),
            isChildAlive: () => true,
            createProfile: ({ runIndex }) => {
                const profile = path.join(root, `deltamod-benchmark-profile-fail-${runIndex}-`);
                fs.mkdirSync(profile);
                return profile;
            },
            removeProfile: profile => fs.rmSync(profile, { recursive: true, force: true }),
            killTree: () => {},
            collectEnvironment: async () => ({ fixture: true }),
            inspectArtifact,
            sleep: async milliseconds => { currentTime += milliseconds; }
        })).rejects.toThrow(/readiness was not observed/);
        expect(fs.existsSync(output)).toBe(false);
    });

    test('publishes JSON exclusively and refuses to overwrite it', () => {
        const root = temporaryDirectory('deltamod-capture-write-');
        const output = path.join(root, 'result.json');
        writeImmutableJson(output, { complete: true });
        expect(JSON.parse(fs.readFileSync(output, 'utf8'))).toEqual({ complete: true });
        expect(() => writeImmutableJson(output, { complete: false }))
            .toThrow(/refusing to overwrite/);
    });
});
