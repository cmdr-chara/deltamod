// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const PROTOCOL_FIELDS = Object.freeze([
    'warmupLaunches',
    'measuredLaunches',
    'profilePolicy',
    'fileCache',
    'readiness',
    'memorySampleWindowMs',
    'memorySampleIntervalMs',
    'memoryAggregation'
]);

const HARDWARE_FIELDS = Object.freeze([
    'operatingSystem',
    'processor',
    'logicalProcessors',
    'physicalMemoryBytes'
]);

function requireFinitePositive(value, label) {
    if (!Number.isFinite(value) || value <= 0) {
        throw new Error(`${label} must be a positive finite number`);
    }
}

function median(values) {
    const sorted = [...values].sort((left, right) => left - right);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 === 0
        ? (sorted[middle - 1] + sorted[middle]) / 2
        : sorted[middle];
}

function nearestRank(values, percentile) {
    const sorted = [...values].sort((left, right) => left - right);
    return sorted[Math.ceil(percentile * sorted.length) - 1];
}

function assertSummaryMatchesSamples(result, label) {
    const ready = result.samples.map((sample, index) => {
        requireFinitePositive(sample?.readyMs, `${label} samples[${index}].readyMs`);
        return sample.readyMs;
    });
    const workingSet = result.samples.map((sample, index) => {
        requireFinitePositive(
            sample?.peakWorkingSetBytes,
            `${label} samples[${index}].peakWorkingSetBytes`
        );
        return sample.peakWorkingSetBytes;
    });
    const expected = {
        readyMinimum: Math.min(...ready),
        readyMedian: median(ready),
        readyP95: nearestRank(ready, 0.95),
        workingSetMedian: median(workingSet),
        workingSetP95: nearestRank(workingSet, 0.95)
    };
    const actual = {
        readyMinimum: result.summary?.readyMs?.minimum,
        readyMedian: result.summary?.readyMs?.median,
        readyP95: result.summary?.readyMs?.p95NearestRank,
        workingSetMedian: result.summary?.peakWorkingSetBytes?.median,
        workingSetP95: result.summary?.peakWorkingSetBytes?.p95NearestRank
    };
    for (const [field, expectedValue] of Object.entries(expected)) {
        if (actual[field] !== expectedValue) {
            throw new Error(`${label} summary mismatch: ${field}`);
        }
    }
}

function assertComparable(baseline, candidate) {
    if (baseline?.schemaVersion !== 1 || candidate?.schemaVersion !== 1) {
        throw new Error('benchmark schemaVersion must be 1');
    }
    if (!['electron', 'tauri'].includes(baseline.runtime)) {
        throw new Error('baseline runtime must be electron or tauri');
    }
    if (candidate.runtime !== 'tauri') {
        throw new Error('candidate runtime must be tauri');
    }

    for (const field of PROTOCOL_FIELDS) {
        if (baseline.protocol?.[field] == null || candidate.protocol?.[field] == null
            || baseline.protocol[field] !== candidate.protocol[field]) {
            throw new Error(`protocol mismatch: ${field}`);
        }
    }
    for (const field of HARDWARE_FIELDS) {
        if (baseline.environment?.[field] == null || candidate.environment?.[field] == null
            || baseline.environment[field] !== candidate.environment[field]) {
            throw new Error(`environment mismatch: ${field}`);
        }
    }

    const expectedSamples = baseline.protocol.measuredLaunches;
    if (!Number.isSafeInteger(expectedSamples) || expectedSamples < 1) {
        throw new Error('measuredLaunches must be a positive integer');
    }
    if (baseline.samples?.length !== expectedSamples) {
        throw new Error('baseline sample count does not match its protocol');
    }
    if (candidate.samples?.length !== expectedSamples) {
        throw new Error('candidate sample count does not match the baseline protocol');
    }

    assertSummaryMatchesSamples(baseline, 'baseline');
    assertSummaryMatchesSamples(candidate, 'candidate');

    requireFinitePositive(baseline.summary?.readyMs?.median, 'baseline ready median');
    requireFinitePositive(candidate.summary?.readyMs?.median, 'candidate ready median');
    requireFinitePositive(
        baseline.summary?.peakWorkingSetBytes?.median,
        'baseline working-set median'
    );
    requireFinitePositive(
        candidate.summary?.peakWorkingSetBytes?.median,
        'candidate working-set median'
    );
    requireFinitePositive(baseline.artifact?.unpackedBytes, 'baseline artifact bytes');
    requireFinitePositive(candidate.artifact?.unpackedBytes, 'candidate artifact bytes');
}

function percentDelta(before, after) {
    return ((after - before) / before) * 100;
}

// Old captures used unpackedBytes for both a directory and a single installer.
// Do not rewrite historical evidence or compare those different storage measures.
function artifactKind(artifact) {
    if (!artifact || !Number.isSafeInteger(artifact.unpackedFileCount)
        || artifact.unpackedFileCount < 1) return 'unknown';
    const locations = [artifact.path, artifact.artifactPath].filter(value => typeof value === 'string');
    const installer = locations.some(value =>
        /\.(?:dmg|deb|msi|rpm|appimage)$/i.test(value)
        || /(?:setup|installer)[^/\\]*\.exe$/i.test(value));
    if (installer) return 'installer-file';
    if (artifact.unpackedFileCount > 1) return 'unpacked-directory';
    return 'single-file';
}

function comparisonMetric(before, after, baselineRuntime) {
    return {
        baseline: before,
        candidate: after,
        // Keep existing Electron comparison consumers compatible. Never label a
        // Tauri baseline as Electron just to reuse the original output schema.
        ...(baselineRuntime === 'electron' ? { electron: before, tauri: after } : {}),
        absoluteDelta: after - before,
        percentDelta: percentDelta(before, after)
    };
}

function compareBenchmarkResults(baseline, candidate) {
    assertComparable(baseline, candidate);
    const metric = (before, after) => comparisonMetric(before, after, baseline.runtime);
    const baselineKind = artifactKind(baseline.artifact);
    const candidateKind = artifactKind(candidate.artifact);
    const comparableStorage = baselineKind === 'unpacked-directory'
        && candidateKind === 'unpacked-directory';
    return {
        schemaVersion: 1,
        baselineRuntime: baseline.runtime,
        candidateRuntime: candidate.runtime,
        baselineRevision: baseline.sourceRevision,
        candidateRevision: candidate.sourceRevision,
        ...(baseline.runtime === 'electron' ? {
            electronRevision: baseline.sourceRevision,
            tauriRevision: candidate.sourceRevision
        } : {}),
        readyMedianMs: metric(
            baseline.summary.readyMs.median,
            candidate.summary.readyMs.median
        ),
        readyP95Ms: metric(
            baseline.summary.readyMs.p95NearestRank,
            candidate.summary.readyMs.p95NearestRank
        ),
        peakWorkingSetMedianBytes: metric(
            baseline.summary.peakWorkingSetBytes.median,
            candidate.summary.peakWorkingSetBytes.median
        ),
        peakWorkingSetP95Bytes: metric(
            baseline.summary.peakWorkingSetBytes.p95NearestRank,
            candidate.summary.peakWorkingSetBytes.p95NearestRank
        ),
        unpackedArtifactBytes: comparableStorage ? metric(
            baseline.artifact.unpackedBytes,
            candidate.artifact.unpackedBytes
        ) : null,
        artifactComparison: {
            comparable: comparableStorage,
            baselineKind,
            candidateKind,
            reason: comparableStorage ? null
                : 'Unpacked size requires two complete unpacked directories, not installers or standalone executables.'
        }
    };
}

if (require.main === module) {
    const [baselineArgument, candidateArgument] = process.argv.slice(2);
    if (!baselineArgument || !candidateArgument) {
        console.error('Usage: node compare.js <baseline-result.json> <tauri-result.json>');
        process.exitCode = 2;
    } else {
        const read = argument => JSON.parse(
            fs.readFileSync(path.resolve(process.cwd(), argument), 'utf8')
        );
        try {
            console.log(JSON.stringify(
                compareBenchmarkResults(read(baselineArgument), read(candidateArgument)),
                null,
                2
            ));
        } catch (error) {
            console.error(error instanceof Error ? error.message : String(error));
            process.exitCode = 1;
        }
    }
}

module.exports = {
    assertComparable,
    compareBenchmarkResults,
    percentDelta
};
