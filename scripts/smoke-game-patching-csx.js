// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const { buildPatchPlan, restore, startGamePatch } = require('../node/GamePatching');
const { loadProvenance, targetForCurrentPlatform, verifyInstallation } = require('./lib/undertale-mod-tool-provenance');

const root = path.resolve(__dirname, '..');
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-game-patching-smoke-'));

function run(executable, args) {
    const result = spawnSync(executable, args, {
        cwd: temporary,
        encoding: 'utf8',
        timeout: 120_000,
        windowsHide: true
    });
    if (result.error || result.status !== 0) {
        throw new Error(`UndertaleModCli ${args[0]} failed with exit code ${result.status}.\n${result.stderr}`);
    }
    return `${result.stdout}\n${result.stderr}`;
}

async function main() {
    const provenance = loadProvenance(root);
    const executable = verifyInstallation(root, provenance, targetForCurrentPlatform());
    const game = path.join(temporary, 'game');
    const mods = path.join(temporary, 'mods');
    const mod = path.join(mods, 'Mod_smoke');
    const scripts = path.join(mod, 'scripts');
    fs.mkdirSync(game, { recursive: true });
    fs.mkdirSync(scripts, { recursive: true });
    const dataFile = path.join(game, 'data.win');
    run(executable, ['new', '--output', dataFile]);
    const originalHash = crypto.createHash('sha256').update(fs.readFileSync(dataFile)).digest('hex');

    fs.writeFileSync(path.join(mod, 'meta.toml'), '[metadata]\nname="CSX integration smoke"\n', 'utf8');
    fs.writeFileSync(path.join(mod, '__deltaID.json'), '{"uniqueId":"csx-integration-smoke"}', 'utf8');
    fs.writeFileSync(
        path.join(mod, 'modding.xml'),
        '<mod><patch type="csx" patch="scripts/patch.csx" to="data.win"/></mod>',
        'utf8'
    );
    fs.writeFileSync(path.join(scripts, 'name.txt'), 'DeltamodGamePatchingSmoke\n', 'utf8');
    fs.writeFileSync(
        path.join(scripts, 'patch.csx'),
        'Data.GeneralInfo.Name.Content = System.IO.File.ReadAllText(System.IO.Path.Combine(System.IO.Path.GetDirectoryName(ScriptPath), "name.txt")).Trim();\n',
        'utf8'
    );

    const approvedPlan = buildPatchPlan(game, mods, ['csx-integration-smoke']);
    const result = await startGamePatch(game, mods, ['csx-integration-smoke'], null, null, {
        approvedPlan,
        undertaleModCliPath: executable,
        timeoutMs: 120_000
    });
    if (!result.patched) throw new Error(`Real GamePatching CSX smoke failed: ${result.log}`);
    if (!run(executable, ['info', dataFile]).includes('DeltamodGamePatchingSmoke')) {
        throw new Error('Real GamePatching CSX smoke did not apply its companion-resource change.');
    }

    restore(game);
    const restoredHash = crypto.createHash('sha256').update(fs.readFileSync(dataFile)).digest('hex');
    if (restoredHash !== originalHash) throw new Error('Real GamePatching CSX smoke did not restore the original data exactly.');
    console.log('Real GamePatching CSX staging, companion resources, commit, and restore verified.');
}

main()
    .catch(error => { console.error(error.message); process.exitCode = 1; })
    .finally(() => fs.rmSync(temporary, { recursive: true, force: true }));
