const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
    REQUIRED_EVENT_PRODUCERS,
    assertParity,
    buildParity,
    extractRustChannels,
    readRustSources
} = require('../lib/parity');
const { compare } = require('../compare-contract');

if (!process.env.DELTAMOD_REPO) throw new Error('DELTAMOD_REPO is required');
const repo = path.resolve(process.env.DELTAMOD_REPO);
const paths = {
    preloadPath: path.join(repo, 'web', 'preload.js'),
    rustPath: path.join(repo, 'src-tauri', 'src', 'main.rs'),
    rustSourceRoot: path.join(repo, 'src-tauri', 'src')
};
const report = buildParity(paths);
assert.equal(report.counts.electronInvoke, 128);
assert.equal(report.counts.electronEvents, 18);
assert.equal(report.counts.rustKnown, 128);
assert.equal(report.counts.rustImplemented + report.counts.rustUnsupported, 128);
assert.equal(report.counts.rustImplemented, 122);
assert.equal(report.counts.rustUnsupported, 6);
assert.equal(report.excludedInternal.length, 5);
assert.equal(report.gaps.missingFromRust.length, 0);
assert.equal(report.gaps.rustOnly.length, 0);
assert.deepEqual(report.gaps.missingEventProducers, []);
assert.equal(report.ok, true);
for (const [event, expectedFile] of Object.entries(REQUIRED_EVENT_PRODUCERS)) {
    const producer = report.rust.eventProducers.find(item => item.event === event);
    assert.equal(producer.present, true);
    assert.equal(producer.expectedFile, expectedFile);
    assert.equal(producer.producers.some(candidate => candidate.file === expectedFile), true);
}
const rustSources = readRustSources(paths.rustSourceRoot, repo);
for (const [event, expectedFile] of Object.entries(REQUIRED_EVENT_PRODUCERS)) {
    let deleted = false;
    const mutatedSources = rustSources.map(file => {
        if (file.file !== expectedFile) return file;
        const literal = `"${event}"`;
        assert.equal(file.source.includes(literal), true);
        deleted = true;
        return { ...file, source: file.source.replace(literal, `"deleted-${event}"`) };
    });
    assert.equal(deleted, true);
    const missingProducerReport = buildParity({ ...paths, rustSources: mutatedSources });
    assert.equal(missingProducerReport.ok, false);
    assert.equal(
        missingProducerReport.gaps.missingEventProducers.some(
            missing => missing.event === event && missing.expectedFile === expectedFile
        ),
        true
    );
    assert.throws(() => assertParity(missingProducerReport), /missing Rust event producers/);
}
const rendererEvents = new Set(report.electron.events.map(event => event.name));
assert.equal(rendererEvents.has('leave-controller-mode'), true);
assert.equal(rendererEvents.has('protocol-download-progress'), true);
assert.ok(report.rust.channels.length > 0);
assert.equal(extractRustChannels('impl FromStr for BackendChannel { fn from_str(x: &str) -> Result<Self, ()> { match x { "a" | "b" => Self::Unsupported(x.to_owned()), "c" => Self::Implemented(x.to_owned()), _ => return Err(()) } } }').length, 3);
const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'fixtures', 'contract.json')));
const responses = Object.fromEntries(fixture.cases.map(test => [test.id, { legacy: test.legacy, rust: test.rust }]));
assert.equal(compare(fixture, Object.fromEntries(Object.entries(responses).map(([id, x]) => [id, x.legacy])), Object.fromEntries(Object.entries(responses).map(([id, x]) => [id, x.rust]))).length, 0);
console.log('harness tests ok');
