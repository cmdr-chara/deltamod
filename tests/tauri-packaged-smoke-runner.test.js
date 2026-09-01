const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { execFile } = require('node:child_process');
const { promisify } = require('node:util');

const {
    isProcessAlive,
    runPackagedSmoke
} = require('../scripts/tauri-parity/run-packaged-smoke');

const execFileAsync = promisify(execFile);
const runnerPath = path.join(__dirname, '..', 'scripts', 'tauri-parity', 'run-packaged-smoke.js');

function nodeFixture(source) {
    return ['-e', source];
}

async function waitForProcessGone(pid, timeoutMs = 2_000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        if (!isProcessAlive(pid)) return true;
        await new Promise(resolve => setTimeout(resolve, 25));
    }
    return !isProcessAlive(pid);
}

describe('packaged Tauri smoke runner', () => {
    test('requires a live Node fixture, captures bounded output, and terminates it', async () => {
        const evidence = await runPackagedSmoke({
            executable: process.execPath,
            args: nodeFixture([
                "process.stdout.write('o'.repeat(256));",
                "process.stderr.write('e'.repeat(256));",
                'setInterval(() => {}, 1000);'
            ].join(' ')),
            cwd: path.join(__dirname, '..'),
            timeoutMs: 1_000,
            readinessMs: 100,
            pollMs: 10,
            outputLimitBytes: 32,
            terminationTimeoutMs: 3_000
        });

        expect(evidence.ok).toBe(true);
        expect(evidence.status).toBe('passed');
        expect(evidence.readiness).toMatchObject({
            criterion: 'process-live',
            reached: true,
            requiredForMs: 100
        });
        expect(evidence.readiness.observedForMs).toBeGreaterThanOrEqual(100);
        expect(evidence.output.stdout).toMatchObject({ limitBytes: 32, truncated: true });
        expect(evidence.output.stderr).toMatchObject({ limitBytes: 32, truncated: true });
        expect(evidence.output.stdout.capturedBytes).toBeLessThanOrEqual(32);
        expect(evidence.output.stderr.capturedBytes).toBeLessThanOrEqual(32);
        expect(evidence.output.stdout.totalBytes).toBeGreaterThanOrEqual(256);
        expect(evidence.output.stderr.totalBytes).toBeGreaterThanOrEqual(256);
        expect(evidence.termination).toMatchObject({ requested: true, completed: true });
    });

    test('fails closed when a fixture exits before the live interval', async () => {
        const evidence = await runPackagedSmoke({
            executable: process.execPath,
            args: nodeFixture([
                "process.stdout.write('exited early', () => process.exit(0));"
            ].join(' ')),
            timeoutMs: 1_500,
            readinessMs: 1_000,
            pollMs: 10,
            terminationTimeoutMs: 3_000
        });

        expect(evidence.ok).toBe(false);
        expect(evidence.status).toBe('failed');
        expect(evidence.failure.code, JSON.stringify(evidence, null, 2))
            .toBe('process-exited-before-readiness');
        expect(evidence.readiness.reached).toBe(false);
        expect(evidence.process.exitCode).toBe(0);
        expect(evidence.output.stdout.text).toContain('exited early');
    });

    test('fails at the bounded readiness timeout and still cleans up the live fixture', async () => {
        const evidence = await runPackagedSmoke({
            executable: process.execPath,
            args: nodeFixture('setInterval(() => {}, 1000);'),
            timeoutMs: 100,
            readinessMs: 250,
            pollMs: 10,
            terminationTimeoutMs: 3_000
        });

        expect(evidence.ok).toBe(false);
        expect(evidence.failure.code).toBe('readiness-timeout');
        expect(evidence.readiness.reached).toBe(false);
        expect(evidence.durationMs).toBeLessThan(3_500);
        expect(evidence.termination).toMatchObject({ requested: true, completed: true });
    });

    test('passes the disposable data root to the fixture and writes the same evidence document', async () => {
        const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-packaged-smoke-'));
        const evidencePath = path.join(temporaryRoot, 'evidence.json');
        try {
            const result = await execFileAsync(process.execPath, [
                runnerPath,
                '--executable', process.execPath,
                '--data-root', temporaryRoot,
                '--evidence-file', evidencePath,
                '--timeout-ms', '1000',
                '--ready-for-ms', '80',
                '--poll-ms', '10',
                '--termination-timeout-ms', '3000',
                '--',
                ...nodeFixture([
                    "if (!process.argv.includes('--data-root')) process.exit(7);",
                    "if (process.env.DELTAMOD_SMOKE_DATA_ROOT !== process.argv[process.argv.indexOf('--data-root') + 1]) process.exit(8);",
                    "require('node:fs').writeFileSync(require('node:path').join(process.env.DELTAMOD_SMOKE_DATA_ROOT, 'initialized'), 'ok');",
                    'setInterval(() => {}, 1000);'
                ].join(' ')),
                '--'
            ], {
                cwd: path.join(__dirname, '..'),
                maxBuffer: 1024 * 1024,
                windowsHide: true
            });
            const evidence = JSON.parse(result.stdout);

            expect(evidence.ok, JSON.stringify(evidence, null, 2)).toBe(true);
            expect(evidence.command.dataRoot).toBe(temporaryRoot);
            expect(evidence.command.args.slice(-2)).toEqual(['--data-root', temporaryRoot]);
            expect(evidence.isolation).toMatchObject({
                dataRootRequired: true,
                dataRootObserved: true
            });
            expect(JSON.parse(fs.readFileSync(evidencePath, 'utf8'))).toEqual(evidence);
        } finally {
            fs.rmSync(temporaryRoot, { recursive: true, force: true });
        }
    });

    test('requires bounded in-app capability evidence when requested', async () => {
        const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-capability-smoke-'));
        try {
            const fixture = [
                "const fs = require('node:fs');",
                "const evidence = {schemaVersion:1,status:'passed',ok:true,packageVersion:'2.0.13',checks:{packaged:true,flagSet:true,flagRead:true,flagPersisted:true,baseThemeAvailable:true,baseThemeActive:true,installationListed:true,gameLoaded:true,unknownChannelRejected:true}};",
                "fs.writeFileSync(process.env.DELTAMOD_SMOKE_CAPABILITY_FILE, JSON.stringify(evidence));",
                "setInterval(() => {}, 1000);"
            ].join(' ');
            const evidence = await runPackagedSmoke({
                executable: process.execPath,
                args: [...nodeFixture(fixture), '--'],
                dataRoot: temporaryRoot,
                capabilityProbe: true,
                expectedVersion: '2.0.13',
                timeoutMs: 1_000,
                readinessMs: 80,
                pollMs: 10,
                terminationTimeoutMs: 3_000
            });

            expect(evidence.ok, JSON.stringify(evidence, null, 2)).toBe(true);
            expect(evidence.readiness.criterion).toBe('capability-evidence-and-process-live');
            expect(evidence.capability).toMatchObject({ required: true, observed: true });
            expect(evidence.capability.evidence.checks.unknownChannelRejected).toBe(true);
        } finally {
            fs.rmSync(temporaryRoot, { recursive: true, force: true });
        }
    });

    test('rejects capability evidence from a different packaged version', async () => {
        const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-capability-version-'));
        try {
            const fixture = [
                "const fs = require('node:fs');",
                "const evidence = {schemaVersion:1,status:'passed',ok:true,packageVersion:'9.9.9',checks:{packaged:true,flagSet:true,flagRead:true,flagPersisted:true,baseThemeAvailable:true,baseThemeActive:true,installationListed:true,gameLoaded:true,unknownChannelRejected:true}};",
                "fs.writeFileSync(process.env.DELTAMOD_SMOKE_CAPABILITY_FILE, JSON.stringify(evidence));",
                "setInterval(() => {}, 1000);"
            ].join(' ');
            const evidence = await runPackagedSmoke({
                executable: process.execPath,
                args: [...nodeFixture(fixture), '--'],
                dataRoot: temporaryRoot,
                capabilityProbe: true,
                expectedVersion: '2.0.13',
                timeoutMs: 1_000,
                readinessMs: 60,
                pollMs: 10,
                terminationTimeoutMs: 3_000
            });

            expect(evidence.ok).toBe(false);
            expect(evidence.failure).toMatchObject({ code: 'capability-probe-failed' });
            expect(evidence.failure.message).toContain('9.9.9 instead of 2.0.13');
        } finally {
            fs.rmSync(temporaryRoot, { recursive: true, force: true });
        }
    });

    test('returns nonzero with failure evidence for malformed CLI values', async () => {
        let failure;
        try {
            await execFileAsync(process.execPath, [
                runnerPath,
                '--executable', process.execPath,
                '--timeout-ms', '1,000'
            ], {
                cwd: path.join(__dirname, '..'),
                maxBuffer: 1024 * 1024,
                windowsHide: true
            });
        } catch (error) {
            failure = error;
        }

        expect(failure).toMatchObject({ code: 1, stderr: '' });
        expect(JSON.parse(failure.stdout)).toMatchObject({
            ok: false,
            status: 'failed',
            failure: { code: 'invalid-cli' }
        });
    });

    test('kills the fixture child process tree through the CLI and emits stable JSON', async () => {
        const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-packaged-smoke-'));
        const pidFile = path.join(temporaryRoot, 'child.pid');
        try {
            const childSource = 'setInterval(() => {}, 1000);';
            const parentSource = [
                "const fs = require('node:fs');",
                "const { spawn } = require('node:child_process');",
                `const child = spawn(process.execPath, ['-e', ${JSON.stringify(childSource)}], { stdio: 'ignore' });`,
                `fs.writeFileSync(process.env.SMOKE_PID_FILE, String(child.pid));`,
                'setInterval(() => {}, 1000);'
            ].join(' ');
            const result = await execFileAsync(process.execPath, [
                runnerPath,
                '--executable', process.execPath,
                '--timeout-ms', '1000',
                '--ready-for-ms', '100',
                '--poll-ms', '10',
                '--termination-timeout-ms', '3000',
                '--',
                '-e', parentSource
            ], {
                cwd: path.join(__dirname, '..'),
                env: { ...process.env, SMOKE_PID_FILE: pidFile },
                maxBuffer: 1024 * 1024,
                windowsHide: true
            }).catch(error => {
                throw new Error(`unexpected CLI failure: ${error.stdout || error.message}`);
            });

            const evidence = JSON.parse(result.stdout);
            expect(evidence.ok).toBe(true);
            expect(evidence.status).toBe('passed');
            expect(evidence.command.args).toContain('-e');
            expect(evidence.termination).toMatchObject({
                requested: true,
                completed: true,
                method: process.platform === 'win32' ? 'taskkill' : 'process-group'
            });
            const childPid = Number(fs.readFileSync(pidFile, 'utf8'));
            expect(Number.isInteger(childPid)).toBe(true);
            expect(await waitForProcessGone(childPid)).toBe(true);
            expect(result.stderr).toBe('');
        } finally {
            fs.rmSync(temporaryRoot, { recursive: true, force: true });
        }
    });
});
