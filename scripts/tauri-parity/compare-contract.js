#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const { isDeepStrictEqual } = require('node:util');

const MAX_CASES = 4096;
function load(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function record(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function compare(fixture, legacy, rust, backendChannels) {
  const errors = [];
  if (!record(fixture) || fixture.schemaVersion !== 1 || !Array.isArray(fixture.cases)
      || fixture.cases.length === 0 || fixture.cases.length > MAX_CASES) {
    return ['invalid contract fixture: expected schemaVersion 1 and a bounded, nonempty cases array'];
  }
  if (!record(legacy) || !record(rust)) return ['contract outputs must be objects'];
  const classifications = backendChannels === undefined ? null
    : new Map(backendChannels.map(channel => [channel.name, channel.classification]));
  const ids = new Set();
  for (const test of fixture.cases) {
    if (!record(test) || typeof test.id !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(test.id)
        || typeof test.channel !== 'string' || test.channel.length === 0
        || !Array.isArray(test.data) || typeof test.expectEquivalent !== 'boolean'
        || !Object.hasOwn(test, 'legacy') || !Object.hasOwn(test, 'rust')) {
      errors.push('invalid contract case: required fields are missing or invalid');
      continue;
    }
    if (ids.has(test.id)) { errors.push(`${test.id}: duplicate contract case`); continue; }
    ids.add(test.id);
    if (!Object.hasOwn(legacy, test.id) || !Object.hasOwn(rust, test.id)) {
      errors.push(`${test.id}: missing own output entry`);
      continue;
    }
    const a = legacy[test.id];
    const b = rust[test.id];
    if (!isDeepStrictEqual(a, test.legacy)) errors.push(`${test.id}: legacy output differs from fixture`);
    if (!isDeepStrictEqual(b, test.rust)) errors.push(`${test.id}: Rust output differs from fixture`);
    if (test.expectEquivalent && !isDeepStrictEqual(a, b)) errors.push(`${test.id}: expected legacy/Rust equivalence`);
    if (classifications) {
      const actual = classifications.get(test.channel) || 'unknown';
      if (!['implemented', 'unsupported', 'unknown'].includes(test.classification)) {
        errors.push(`${test.id}: fixture must declare a production classification`);
      } else if (test.classification !== actual) {
        errors.push(`${test.id}: stale classification: fixture=${test.classification}, production=${actual}`);
      }
      if (actual !== 'implemented' && (!record(test.rust) || test.rust.ok !== false
          || typeof test.rust.error !== 'string' || !test.rust.error.startsWith('TAURI_COMMAND_UNAVAILABLE:'))) {
        errors.push(`${test.id}: unavailable channel must not claim success or conceal its unavailable response`);
      }
      if (actual === 'implemented' && record(test.rust)
          && typeof test.rust.error === 'string' && test.rust.error.startsWith('TAURI_COMMAND_UNAVAILABLE:')) {
        errors.push(`${test.id}: implemented channel has a stale unavailable response`);
      }
    }
  }
  for (const [name, output] of [['legacy', legacy], ['Rust', rust]]) {
    for (const id of Object.keys(output)) {
      if (!ids.has(id)) errors.push(`${id}: unexpected ${name} output without a contract case`);
    }
  }
  return errors;
}
if (require.main === module) {
  const [fixtureFile, legacyFile, rustFile, sourceFile] = process.argv.slice(2);
  if (!fixtureFile || !legacyFile || !rustFile) {
    console.error('usage: node compare-contract.js fixture.json legacy.json rust.json [main.rs]');
    process.exit(2);
  }
  try {
    const { extractRustChannels } = require('./lib/parity');
    const fixture = load(fixtureFile);
    const source = sourceFile || path.resolve(__dirname, '../../src-tauri/src/main.rs');
    const channels = extractRustChannels(fs.readFileSync(source, 'utf8'));
    const errors = compare(fixture, load(legacyFile), load(rustFile), channels);
    if (errors.length) throw new Error(errors.join('\n'));
    console.log(`static contract ok: ${fixture.cases.length} cases; production classifications checked (not runtime smoke)`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
module.exports = { compare };
