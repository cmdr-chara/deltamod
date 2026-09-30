// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { launch } from '@gpuix/react/automation';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { createHash } from 'node:crypto';
import { sampleTree, summarize } from './processes.mjs';

const [sourceArgument, outputArgument] = process.argv.slice(2);
if (!sourceArgument || !outputArgument) throw new Error('Usage: npm run benchmark -- <disposable-profile-fixture> <new-output-directory>');
const sourceProfile = fs.realpathSync(sourceArgument);
const output = path.resolve(outputArgument);
if (!fs.statSync(sourceProfile).isDirectory()) throw new Error('A profile fixture directory is required.');
if (fs.existsSync(output)) throw new Error('Refusing to overwrite benchmark evidence.');
const root = fileURLToPath(new URL('..', import.meta.url));
const entry = path.join(root, 'dist', 'main.js');
if (!fs.existsSync(entry)) throw new Error('Build the GPUIX frontend first.');
for (const lock of ['package-lock.json', 'native/Cargo.lock']) {
  if (!fs.existsSync(path.join(root, lock))) throw new Error(`Resolve and freeze ${lock} before collecting native measurements.`);
}
fs.mkdirSync(output, { recursive: true });
const samples = [];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function waitForMarker(file) {
  const end = performance.now() + 30000;
  while (!fs.existsSync(file)) {
    if (performance.now() >= end) throw new Error('Native model readiness timed out.');
    await sleep(50);
  }
  const marker = JSON.parse(fs.readFileSync(file, 'utf8'));
  if (marker.phase !== 'model-and-react-commit' || marker.sourceAttached !== true || !Number.isSafeInteger(marker.pid) || marker.pid <= 0) {
    throw new Error('Invalid native readiness marker.');
  }
  return marker;
}
try {
  for (let run = 0; run < 8; run++) {
    const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-gpuix-bench-'));
    const markerFile = path.join(temporary, 'ready.json');
    let app;
    try {
      const startedAt = performance.now();
      app = await launch({ command: process.execPath, args: [entry, '--state-root', path.join(temporary, 'state'),
        '--source-profile', sourceProfile, '--no-focus', '--reduce-motion', '--opaque', '--benchmark-file', markerFile] });
      const marker = await waitForMarker(markerFile);
      const ready = await app.getByTestId('home-ready').waitFor();
      if (!ready.bounds || ready.bounds.width <= 0 || ready.bounds.height <= 0) throw new Error('Home has no painted bounds.');
      await app.screenshot({ path: path.join(output, `home-${run}.png`) });
      const paintedHomeObservedMs = performance.now() - startedAt;
      if (run > 0) {
        const observed = [];
        const startMemory = performance.now();
        for (let sample = 0; sample < 20; sample++) {
          const target = startMemory + sample * 100;
          if (target > performance.now()) await sleep(target - performance.now());
          const start = performance.now();
          const memory = await sampleTree(marker.pid);
          observed.push({ ...memory, offsetMs: start - startMemory, queryMs: performance.now() - start });
        }
        samples.push({ paintedHomeObservedMs, sampledPeakBytes: Math.max(...observed.map(item => item.bytes)),
          actualMemoryWindowMs: performance.now() - startMemory, observations: observed });
      }
    } finally {
      try { if (app) await app.close(); }
      finally { fs.rmSync(temporary, { recursive: true, force: true, maxRetries: 3 }); }
    }
  }
  const hashes = Object.fromEntries(['package-lock.json', 'native/Cargo.lock'].map(file => [file,
    createHash('sha256').update(fs.readFileSync(path.join(root, file))).digest('hex')]));
  fs.writeFileSync(path.join(output, 'gpuix.json'), JSON.stringify({
    schemaVersion: 1, runtime: 'gpuix-react', capturedAt: new Date().toISOString(),
    environment: { os: os.type(), release: os.release(), arch: os.arch(), processor: os.cpus()[0]?.model, node: process.version },
    protocol: { warmupLaunches: 1, measuredLaunches: 7, readiness: 'native-home-bounds-and-gpu-readback-v1',
      profilePolicy: 'fresh-preview-state-read-only-source-fixture', memory: process.platform === 'win32' ? 'sum-process-working-sets' : 'sum-process-rss',
      nominalWindowMs: 2000, nominalIntervalMs: 100, reducedMotion: true, opaque: true },
    dependencyLockHashes: hashes, samples,
    summary: { paintedHomeObservedMs: summarize(samples, 'paintedHomeObservedMs'), sampledPeakBytes: summarize(samples, 'sampledPeakBytes') },
    comparableToHistoricalTauri: false,
    limitations: ['Read-only prototype, not full application feature parity.', 'Readiness includes automation polling and GPU screenshot readback.',
      'Actual memory-query timing is retained. Queries can stretch the nominal window.', 'RSS/working-set sums can count shared pages more than once.'],
  }, null, 2) + '\n', { flag: 'wx' });
} catch (error) {
  fs.writeFileSync(path.join(output, 'failure.json'), JSON.stringify({ error: String(error.message || error), completedSamples: samples }, null, 2));
  throw error;
}
