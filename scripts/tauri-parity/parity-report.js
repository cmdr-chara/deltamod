#!/usr/bin/env node
const path = require('node:path');
const { buildParity, assertParity } = require('./lib/parity');

const repoArg = process.argv[2] || process.env.DELTAMOD_REPO;
if (!repoArg) { console.error('usage: node parity-report.js <repo> [output.json]'); process.exit(2); }
const repo = path.resolve(repoArg);
const report = buildParity({ preloadPath: path.join(repo, 'web', 'preload.js'), rustPath: path.join(repo, 'src-tauri', 'src', 'main.rs') });
const output = process.argv[3];
if (output) require('node:fs').writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ counts: report.counts, gaps: report.gaps, ok: report.ok }, null, 2));
try { assertParity(report); } catch (error) { console.error(error.message); process.exitCode = 1; }
