'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');

const PROTOCOL_URI = 'deltamod-community://gb/launch?item=12';

function option(name) {
    const index = process.argv.indexOf(name);
    if (index < 0 || index + 1 >= process.argv.length) return null;
    return process.argv[index + 1];
}

function requireFreshDirectory(directory) {
    fs.mkdirSync(directory, { recursive: true });
    const resolved = fs.realpathSync(directory);
    const stat = fs.lstatSync(resolved);
    if (!stat.isDirectory() || stat.isSymbolicLink()) {
        throw new Error('The protocol smoke data root must be a real directory.');
    }
    return resolved;
}

function readJson(file) {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
}

async function waitForJson(file, application, deadline) {
    while (Date.now() < deadline) {
        if (application.spawnError) {
            throw new Error(`The installed application could not start: ${application.spawnError.message}`);
        }
        if (application.exitCode !== null) {
            throw new Error(`The installed application exited early with code ${application.exitCode}.`);
        }
        if (fs.existsSync(file)) return readJson(file);
        await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw new Error(`Timed out waiting for ${path.basename(file)}.`);
}

function runCommand(command, args) {
    const result = spawnSync(command, args, {
        encoding: 'utf8',
        shell: false,
        timeout: 30_000,
        maxBuffer: 16 * 1024
    });
    if (result.error || result.status !== 0) {
        throw new Error(`${command} could not dispatch the installed protocol.`);
    }
    return result.stdout.trim();
}

function linuxDesktopEntry(handler) {
    const dataRoots = [
        process.env.XDG_DATA_HOME || path.join(process.env.HOME || '', '.local', 'share'),
        ...(process.env.XDG_DATA_DIRS || '/usr/local/share:/usr/share').split(':')
    ].filter(Boolean);
    for (const root of dataRoots) {
        const candidate = path.join(root, 'applications', handler);
        if (fs.existsSync(candidate)) return candidate;
    }
    throw new Error(`The registered Linux desktop handler ${handler} is not installed.`);
}

function verifyRegistration(platform, appBundle, executable) {
    if (platform === 'linux') {
        const handler = runCommand('xdg-mime', [
            'query',
            'default',
            'x-scheme-handler/deltamod-community'
        ]);
        if (!handler.endsWith('.desktop')) {
            throw new Error('The installed Linux package did not register a desktop protocol handler.');
        }
        const desktopEntry = linuxDesktopEntry(handler);
        const contents = fs.readFileSync(desktopEntry, 'utf8');
        const execMatch = contents.match(/^Exec="([^"]+)" %u\s*$/m);
        if (!execMatch || fs.realpathSync(execMatch[1]) !== executable) {
            throw new Error('The installed Linux desktop handler does not target the installed executable.');
        }
        return { handler, command: execMatch[1] };
    }

    const plist = path.join(appBundle, 'Contents', 'Info.plist');
    const parsed = JSON.parse(runCommand('plutil', ['-convert', 'json', '-o', '-', plist]));
    const schemes = (parsed.CFBundleURLTypes ?? [])
        .flatMap(entry => entry.CFBundleURLSchemes ?? []);
    if (!schemes.includes('deltamod-community')) {
        throw new Error('The installed macOS bundle does not declare the Deltamod protocol.');
    }
    return 'CFBundleURLTypes';
}

function dispatchProtocol(platform, appBundle, registration) {
    if (platform === 'linux') {
        // GitHub's headless Linux runner has no desktop session, so xdg-open's
        // generic fallback cannot route custom URI schemes. The registration
        // check above resolves and validates the installed desktop entry; run
        // that exact command to exercise the real second-instance handoff.
        runCommand(registration.command, [PROTOCOL_URI]);
        return;
    }
    runCommand('open', ['-a', appBundle, PROTOCOL_URI]);
}

async function stopApplication(application) {
    if (application.exitCode !== null) return;
    application.kill('SIGTERM');
    await Promise.race([
        new Promise(resolve => application.once('exit', resolve)),
        new Promise(resolve => setTimeout(resolve, 5_000))
    ]);
    if (application.exitCode === null) application.kill('SIGKILL');
}

async function run() {
    const executableArgument = option('--executable');
    const dataRootArgument = option('--data-root');
    const evidenceArgument = option('--evidence-file');
    const expectedVersion = option('--expected-version');
    const platform = option('--platform');
    const appBundleArgument = option('--app-bundle');
    const timeoutMs = Number(option('--timeout-ms') ?? 30_000);
    if (!executableArgument || !dataRootArgument || !evidenceArgument || !expectedVersion
        || !['linux', 'macos'].includes(platform) || !Number.isSafeInteger(timeoutMs)
        || timeoutMs < 1_000 || timeoutMs > 120_000) {
        throw new Error('Invalid installed Unix protocol smoke arguments.');
    }
    if ((platform === 'linux' && process.platform !== 'linux')
        || (platform === 'macos' && process.platform !== 'darwin')) {
        throw new Error(`Requested ${platform} protocol smoke on ${process.platform}.`);
    }

    const executable = fs.realpathSync(executableArgument);
    fs.accessSync(executable, fs.constants.X_OK);
    const dataRoot = requireFreshDirectory(path.resolve(dataRootArgument));
    const evidenceFile = path.resolve(evidenceArgument);
    if (platform === 'macos' && !appBundleArgument) {
        throw new Error('The macOS protocol smoke requires --app-bundle.');
    }
    const appBundle = platform === 'macos' ? fs.realpathSync(appBundleArgument) : null;
    if (platform === 'macos' && !appBundle.startsWith('/Applications/')) {
        throw new Error('The macOS deep-link smoke requires an installed /Applications bundle.');
    }

    const capabilityFile = path.join(dataRoot, '.deltamod-capability-evidence.json');
    const queueFile = path.join(dataRoot, '.deltamod-protocol-queue-evidence.json');
    const protocolFile = path.join(dataRoot, '.deltamod-protocol-evidence.json');
    if ([capabilityFile, queueFile, protocolFile].some(file => fs.existsSync(file))) {
        throw new Error('The protocol smoke requires a fresh disposable data root.');
    }

    const application = spawn(executable, ['--data-root', dataRoot], {
        env: {
            ...process.env,
            DELTAMOD_SMOKE_DATA_ROOT: dataRoot,
            DELTAMOD_SMOKE_CAPABILITY_FILE: capabilityFile,
            DELTAMOD_SMOKE_PROTOCOL_FILE: protocolFile
        },
        detached: false,
        shell: false,
        stdio: 'ignore'
    });
    application.spawnError = null;
    application.once('error', error => {
        application.spawnError = error;
    });

    try {
        const deadline = Date.now() + timeoutMs;
        const capability = await waitForJson(capabilityFile, application, deadline);
        if (capability.ok !== true || capability.packageVersion !== expectedVersion) {
            throw new Error('The installed capability probe returned an unexpected version or status.');
        }
        await new Promise(resolve => setTimeout(resolve, 1_500));

        const registration = verifyRegistration(platform, appBundle, executable);
        dispatchProtocol(platform, appBundle, registration);
        const queued = await waitForJson(queueFile, application, deadline);
        const protocol = await waitForJson(protocolFile, application, deadline);
        const checks = {
            registeredHandler: Boolean(registration),
            queuedInFirstProcess: Number(queued.processId) === application.pid,
            forwardedToFirstProcess: Number(protocol.processId) === application.pid,
            rendererReady: protocol.checks?.rendererReady === true,
            strictProtocolAction: protocol.checks?.strictProtocolAction === true,
            expectedAction: protocol.action === 'launch' && Number(protocol.itemId) === 12,
            firstProcessStillRunning: application.exitCode === null
        };
        if (Object.values(checks).includes(false)) {
            throw new Error('The installed protocol smoke returned a failed check.');
        }

        const evidence = {
            schemaVersion: 1,
            status: 'passed',
            ok: true,
            packageVersion: protocol.packageVersion,
            platform,
            checks,
            operationId: protocol.operationId,
            rendererGeneration: protocol.rendererGeneration
        };
        fs.mkdirSync(path.dirname(evidenceFile), { recursive: true });
        fs.writeFileSync(evidenceFile, `${JSON.stringify(evidence, null, 2)}\n`, 'utf8');
        process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
    } finally {
        await stopApplication(application);
    }
}

run().catch(error => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
});
