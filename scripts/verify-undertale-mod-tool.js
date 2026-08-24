// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.
// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

const path = require('path');
const { loadProvenance, targetForCurrentPlatform, verifyInstallation } = require('./lib/undertale-mod-tool-provenance');
const { smokeUndertaleModCli } = require('./lib/undertale-mod-tool-smoke');

const root = path.resolve(__dirname, '..');
const provenance = loadProvenance(root);
if (process.argv.includes('--manifest-only')) {
    console.log('UndertaleModTool source, license, release URLs, and checksums are pinned.');
    process.exit(0);
}
const target = process.argv.slice(2).find(argument => !argument.startsWith('-')) || targetForCurrentPlatform();
const executable = verifyInstallation(root, provenance, target);
if (process.argv.includes('--tree-only')) {
    console.log(`UndertaleModCli ${target} complete installation tree verified.`);
    process.exit(0);
}
smokeUndertaleModCli(executable);
console.log(`UndertaleModCli ${target} full-tree integrity and real CSX execution verified.`);
