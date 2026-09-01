// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');
const {
    assertComparable,
    compareBenchmarkResults
} = require('../scripts/desktop-benchmark/compare');

const benchmarkDirectory = path.join(__dirname, '..', 'benchmarks', 'desktop');
const baselinePath = path.join(benchmarkDirectory, 'electron-9e6f8af.json');

function nearestRank(values, percentile) {
    const sorted = [...values].sort((left, right) => left - right);
    return sorted[Math.ceil(percentile * sorted.length) - 1];
}

function median(values) {
    const sorted = [...values].sort((left, right) => left - right);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? (sorted[middle - 1] + sorted[middle]) / 2
        : sorted[middle];
}

describe('desktop runtime benchmark gate', () => {
    test('keeps the Electron baseline comparable and internally consistent', () => {
        const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));

        expect(baseline).toMatchObject({
            schemaVersion: 1,
            runtime: 'electron',
            sourceRevision: '9e6f8af45bff52e7e2c710a92bb18e826fc897dd',
            includesRevision: 'a882423',
            protocol: {
                warmupLaunches: 1,
                measuredLaunches: 7,
                profilePolicy: 'fresh-per-launch',
                fileCache: 'warm',
                readiness: 'first-window-and-main-route-guard-cleared',
                memorySampleWindowMs: 2000,
                memorySampleIntervalMs: 100,
                memoryAggregation: 'sum-of-application-process-working-sets'
            },
            postRewriteComparison: null
        });

        expect(baseline.samples).toHaveLength(baseline.protocol.measuredLaunches);
        for (const sample of baseline.samples) {
            expect(Number.isFinite(sample.readyMs)).toBe(true);
            expect(Number.isSafeInteger(sample.peakWorkingSetBytes)).toBe(true);
            expect(sample.readyMs).toBeGreaterThan(0);
            expect(sample.peakWorkingSetBytes).toBeGreaterThan(0);
        }

        const readyValues = baseline.samples.map(sample => sample.readyMs);
        const workingSetValues = baseline.samples.map(sample => sample.peakWorkingSetBytes);

        expect(baseline.summary.readyMs).toEqual({
            minimum: Math.min(...readyValues),
            median: median(readyValues),
            p95NearestRank: nearestRank(readyValues, 0.95)
        });
        expect(baseline.summary.peakWorkingSetBytes.median)
            .toBe(median(workingSetValues));
        expect(baseline.summary.peakWorkingSetBytes.p95NearestRank)
            .toBe(nearestRank(workingSetValues, 0.95));
        expect(baseline.summary.peakWorkingSetBytes.medianMiB)
            .toBeCloseTo(median(workingSetValues) / 1024 / 1024, 2);
        expect(baseline.summary.peakWorkingSetBytes.p95MiB)
            .toBeCloseTo(nearestRank(workingSetValues, 0.95) / 1024 / 1024, 2);

        expect(baseline.artifact.unpackedFileCount).toBeGreaterThan(0);
        expect(baseline.artifact.unpackedBytes).toBeGreaterThan(0);
        expect(baseline.artifact.appAsarBytes).toBeGreaterThan(0);
        expect(baseline.artifact.executableBytes).toBeGreaterThan(0);
    });

    test('documents that post-rewrite results require the same release-gated protocol', () => {
        const documentation = fs.readFileSync(
            path.join(benchmarkDirectory, 'README.md'),
            'utf8'
        );
        const normalizedDocumentation = documentation.replace(/\s+/g, ' ');

        expect(normalizedDocumentation).toContain('same Windows host and power profile');
        expect(normalizedDocumentation).toContain('one unreported launch');
        expect(normalizedDocumentation).toContain('seven measured launches');
        expect(normalizedDocumentation).toContain('fresh application profile');
        expect(normalizedDocumentation).toContain('production Tauri/Rust');
        expect(normalizedDocumentation).toContain('post-rewrite results stay empty');
    });

    test('compares only a Tauri result captured with the same protocol and hardware', () => {
        const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
        const candidate = structuredClone(baseline);
        candidate.runtime = 'tauri';
        candidate.sourceRevision = 'f'.repeat(40);
        candidate.samples = candidate.samples.map(() => ({
            readyMs: 1000,
            peakWorkingSetBytes: 400_000_000
        }));
        candidate.summary.readyMs = {
            minimum: 1000,
            median: 1000,
            p95NearestRank: 1000
        };
        candidate.summary.peakWorkingSetBytes = {
            median: 400_000_000,
            medianMiB: 381.47,
            p95NearestRank: 400_000_000,
            p95MiB: 381.47
        };
        candidate.artifact.unpackedBytes = 100_000_000;

        const comparison = compareBenchmarkResults(baseline, candidate);
        expect(comparison.readyMedianMs).toMatchObject({
            electron: 1513.9619,
            tauri: 1000,
            absoluteDelta: 1000 - 1513.9619
        });
        expect(comparison.peakWorkingSetMedianBytes.percentDelta).toBeLessThan(0);
        expect(comparison.unpackedArtifactBytes.percentDelta).toBeLessThan(0);

        candidate.protocol.readiness = 'process-created';
        expect(() => assertComparable(baseline, candidate))
            .toThrow('protocol mismatch: readiness');
    });
});
