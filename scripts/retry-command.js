// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const DEFAULT_ATTEMPTS = 3;
const DEFAULT_DELAY_MS = 5_000;

function positiveInteger(value, option, { allowZero = false } = {}) {
    const parsed = Number(value);
    const minimum = allowZero ? 0 : 1;
    if (!Number.isSafeInteger(parsed) || parsed < minimum) {
        throw new Error(`${option} must be an integer greater than or equal to ${minimum}.`);
    }
    return parsed;
}

function parseArguments(argv) {
    const separator = argv.indexOf('--');
    if (separator === -1 || separator === argv.length - 1) {
        throw new Error('Usage: retry-command [--attempts N] [--delay-ms N] [--label TEXT] -- COMMAND [ARGS...]');
    }

    const options = argv.slice(0, separator);
    const command = argv[separator + 1];
    const args = argv.slice(separator + 2);
    let attempts = DEFAULT_ATTEMPTS;
    let delayMs = DEFAULT_DELAY_MS;
    let label = [command, ...args].join(' ');

    for (let index = 0; index < options.length; index += 1) {
        const option = options[index];
        const value = options[index + 1];
        if (!value) throw new Error(`${option} requires a value.`);

        if (option === '--attempts') attempts = positiveInteger(value, option);
        else if (option === '--delay-ms') delayMs = positiveInteger(value, option, { allowZero: true });
        else if (option === '--label') label = value;
        else throw new Error(`Unknown retry-command option: ${option}`);
        index += 1;
    }

    return { attempts, delayMs, label, command, args };
}

function resolveCommand(command, args, {
    platform = process.platform,
    nodeExecutable = process.execPath,
    existsSync = fs.existsSync
} = {}) {
    if (platform !== 'win32') return { command, args };

    const pathApi = path.win32;
    const commandName = pathApi.basename(command).toLowerCase().replace(/\.cmd$/, '');
    if (commandName !== 'npm' && commandName !== 'npx') return { command, args };

    const cliName = commandName === 'npm' ? 'npm-cli.js' : 'npx-cli.js';
    const cliPath = pathApi.join(pathApi.dirname(nodeExecutable), 'node_modules', 'npm', 'bin', cliName);
    if (!existsSync(cliPath)) {
        throw new Error(`Could not locate the ${commandName} CLI beside Node.js: ${cliPath}`);
    }
    return { command: nodeExecutable, args: [cliPath, ...args] };
}

function safeAnnotation(value) {
    return String(value).replace(/[\r\n]+/g, ' ').replace(/::/g, ':');
}

function wait(delayMs) {
    return new Promise(resolve => setTimeout(resolve, delayMs));
}

async function runWithRetry(configuration, {
    spawn = spawnSync,
    sleep = wait,
    logger = console,
    platform = process.platform,
    nodeExecutable = process.execPath,
    existsSync = fs.existsSync
} = {}) {
    const resolved = resolveCommand(configuration.command, configuration.args, {
        platform,
        nodeExecutable,
        existsSync
    });
    const label = safeAnnotation(configuration.label);
    let lastExitCode = 1;

    for (let attempt = 1; attempt <= configuration.attempts; attempt += 1) {
        const result = spawn(resolved.command, resolved.args, {
            stdio: 'inherit',
            windowsHide: true,
            shell: false
        });
        if (result.status === 0) return 0;

        lastExitCode = Number.isInteger(result.status) && result.status > 0 ? result.status : 1;
        if (result.error) logger.error(safeAnnotation(result.error.message));
        if (attempt >= configuration.attempts) break;

        const retryDelayMs = configuration.delayMs * attempt;
        logger.warn(`::warning::${label} failed on attempt ${attempt}/${configuration.attempts}; retrying in ${retryDelayMs} ms.`);
        await sleep(retryDelayMs);
    }

    logger.error(`::error::${label} failed after ${configuration.attempts} attempts.`);
    return lastExitCode;
}

async function main(argv = process.argv.slice(2)) {
    const configuration = parseArguments(argv);
    process.exitCode = await runWithRetry(configuration);
}

if (require.main === module) {
    main().catch(error => {
        console.error(safeAnnotation(error.message));
        process.exitCode = 1;
    });
}

module.exports = {
    parseArguments,
    resolveCommand,
    runWithRetry,
    safeAnnotation
};
