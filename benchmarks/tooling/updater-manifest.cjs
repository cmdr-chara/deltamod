// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
// Synthetic tooling benchmark only. Run from a trusted checkout with full history:
// node --expose-gc benchmarks/tooling/updater-manifest.cjs <baseline-ref>
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { performance } = require('node:perf_hooks');
const { execFileSync, spawnSync } = require('node:child_process');
const tag = 'community-v2.0.18';
const names = ['Deltamod Community_2.0.18_x64-setup.exe', 'Deltamod Community_2.0.18_x64.app.tar.gz', 'Deltamod Community_2.0.18_aarch64.app.tar.gz'];
function fixture() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-manifest-bench-'));
    for (const name of names) {
        fs.writeFileSync(path.join(dir, name), 'not a real updater artifact');
        fs.writeFileSync(path.join(dir, `${name}.sig`), 'not a cryptographic signature');
    }
    return dir;
}
function measure(fn) {
    for (let i = 0; i < 30; i++) fn();
    const samples = [];
    for (let sample = 0; sample < 9; sample++) {
        if (global.gc) global.gc();
        const start = performance.now();
        for (let i = 0; i < 100; i++) fn();
        samples.push((performance.now() - start) / 100);
    }
    return { iterationsPerSample: 100, samplesMsPerCall: samples, medianMsPerCall: [...samples].sort((a, b) => a - b)[4] };
}
if (process.argv[2] === '--oversize') {
    const implementation = require(process.argv[3]).generate;
    const dir = fixture();
    try {
        const fd = fs.openSync(path.join(dir, `${names[0]}.sig`), 'w');
        try { fs.ftruncateSync(fd, 32 * 1024 * 1024); } finally { fs.closeSync(fd); }
        const start = performance.now();
        let rejected = false;
        try { implementation(dir, tag); } catch { rejected = true; }
        console.log(JSON.stringify({ rejected, milliseconds: performance.now() - start, peakRssKiB: process.resourceUsage().maxRSS }));
    } finally { fs.rmSync(dir, { recursive: true, force: true }); }
} else {
    const root = path.resolve(__dirname, '../..');
    const ref = process.argv[2] || 'd451315768e9043cdf5b2b635398a2350f3f1745';
    const sourcePath = 'scripts/generate-tauri-updater-manifest.js';
    const source = execFileSync('git', ['show', `${ref}:${sourcePath}`], { cwd: root });
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-baseline-'));
    const dir = fixture();
    try {
        const baseline = path.join(scratch, 'baseline.cjs');
        fs.writeFileSync(baseline, source);
        const candidate = path.join(root, sourcePath);
        const blob = bytes => crypto.createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
        const result = {
            schemaVersion: 1,
            scope: 'Synthetic unsigned updater-manifest tooling only. Not desktop, Tauri, installer or cryptographic verification.',
            baselineGitBlob: blob(source), candidateGitBlob: blob(fs.readFileSync(candidate)),
            environment: { node: process.version, platform: process.platform, arch: process.arch, cpu: os.cpus()[0].model },
            validThreeTargetManifest: {}, oversizedSignature32MiB: {}
        };
        for (const [name, modulePath] of [['baseline', baseline], ['candidate', candidate]]) {
            result.validThreeTargetManifest[name] = measure(() => require(modulePath).generate(dir, tag));
            const child = spawnSync(process.execPath, [__filename, '--oversize', modulePath], { encoding: 'utf8', timeout: 15000 });
            if (child.error || child.signal || child.status !== 0) throw new Error(child.stderr || 'Benchmark child failed.');
            result.oversizedSignature32MiB[name] = JSON.parse(child.stdout);
        }
        console.log(JSON.stringify(result, null, 2));
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
        fs.rmSync(scratch, { recursive: true, force: true });
    }
}
