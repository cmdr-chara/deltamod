// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const fs = require('node:fs');
const path = require('node:path');

function verify(root) {
    const errors = [];
    const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
    const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
    if (pkg.main || pkg.build) errors.push('Obsolete desktop entry point or builder configuration.');
    const isElectron = name => /^(?:@electron\/|electron(?:$|-)|app-builder(?:$|-)|builder-util(?:$|-))/.test(name);
    for (const group of ['dependencies', 'devDependencies', 'optionalDependencies']) {
        for (const name of Object.keys(pkg[group] || {})) if (isElectron(name)) errors.push(`Obsolete dependency: ${name}`);
    }
    for (const key of Object.keys(lock.packages || {})) {
        const name = key.split('node_modules/').at(-1);
        if (name && isElectron(name)) errors.push(`Obsolete lockfile package: ${name}`);
    }
    for (const relative of ['node/Runner.js', 'node/IPCHandlers.js', 'web/preload.js', 'web/dlmodal/preload.js',
        'web/views/electron-tracer', 'scripts/legacy', '.github/workflows/release.yml']) {
        if (fs.existsSync(path.join(root, relative))) errors.push(`Obsolete runtime path: ${relative}`);
    }
    const runtimeImport = /(?:require\s*\(\s*|from\s*|import\s*\(\s*)['"](?:electron|electron-updater|electron-builder)['"]/;
    function scan(relative) {
        const directory = path.join(root, relative);
        for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
            const child = path.join(relative, entry.name);
            if (entry.isDirectory()) scan(child);
            else if (entry.isFile() && /\.(?:js|ts|tsx|cjs|mjs)$/.test(entry.name)) {
                const source = fs.readFileSync(path.join(root, child), 'utf8');
                if (runtimeImport.test(source) || /\b(?:window|root)\.electronAPI\b/.test(source)) errors.push(`Obsolete runtime binding: ${child}`);
            }
        }
    }
    for (const directory of ['node', 'web']) scan(directory);
    return errors;
}
if (require.main === module) {
    const errors = verify(path.resolve(__dirname, '..'));
    if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
    else console.log('Tauri-only runtime, renderer and dependency checks passed. Historical evidence is retained.');
}
module.exports = { verify };
