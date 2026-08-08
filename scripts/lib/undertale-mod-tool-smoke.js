// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

function smokeUndertaleModCli(executable) {
    const help = spawnSync(executable, ['load', '--help'], { encoding: 'utf8', timeout: 60_000, windowsHide: true });
    const helpOutput = `${help.stdout}\n${help.stderr}`;
    if (help.error || help.status !== 0 || !helpOutput.includes('--scripts') || !helpOutput.includes('--output')) {
        throw new Error(`Verified UndertaleModCli failed its script CLI help test with exit code ${help.status}.`);
    }

    const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-utmt-smoke-'));
    try {
        const input = path.join(temporary, 'input.win');
        const output = path.join(temporary, 'output.win');
        const script = path.join(temporary, 'smoke.csx');
        fs.writeFileSync(script, 'Data.GeneralInfo.Name.Content = "DeltamodCsxSmoke";\n', 'utf8');
        const create = spawnSync(executable, ['new', '--output', input], {
            cwd: temporary,
            encoding: 'utf8',
            timeout: 60_000,
            windowsHide: true
        });
        if (create.error || create.status !== 0) {
            throw new Error(`UndertaleModCli could not create smoke-test data (exit ${create.status}).`);
        }
        const patch = spawnSync(executable, ['load', input, '--output', output, '--scripts', script], {
            cwd: temporary,
            encoding: 'utf8',
            timeout: 120_000,
            windowsHide: true
        });
        if (patch.error || patch.status !== 0 || !fs.existsSync(output)) {
            throw new Error(`UndertaleModCli could not execute the smoke-test CSX (exit ${patch.status}).`);
        }
        const header = Buffer.alloc(4);
        const handle = fs.openSync(output, 'r');
        try { fs.readSync(handle, header, 0, header.length, 0); } finally { fs.closeSync(handle); }
        if (header.toString('ascii') !== 'FORM') throw new Error('UndertaleModCli smoke-test output is not GameMaker data.');
        const info = spawnSync(executable, ['info', output], {
            cwd: temporary,
            encoding: 'utf8',
            timeout: 60_000,
            windowsHide: true
        });
        if (info.error || info.status !== 0 || !`${info.stdout}\n${info.stderr}`.includes('DeltamodCsxSmoke')) {
            throw new Error('UndertaleModCli smoke-test output did not contain the scripted change.');
        }
    } finally {
        fs.rmSync(temporary, { recursive: true, force: true });
    }
}

module.exports = { smokeUndertaleModCli };
