// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const path = require('path');
const { loadProvenance, verifyPackagedInstallation } = require('./lib/undertale-mod-tool-provenance');
const { smokeUndertaleModCli } = require('./lib/undertale-mod-tool-smoke');

const root = path.resolve(__dirname, '..');
const packageRoot = path.resolve(process.argv[2] || '');
const target = process.argv[3];
if (!process.argv[2] || !target) {
    throw new Error('Usage: verify-packaged-undertale-mod-tool <package-root> <target>');
}

const provenance = loadProvenance(root);
const executable = verifyPackagedInstallation(packageRoot, provenance, target);
if (!process.argv.includes('--tree-only')) smokeUndertaleModCli(executable);
console.log(`Packaged UndertaleModCli ${target} complete tree verified at ${executable}.`);
