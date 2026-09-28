// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const { test } = require('node:test');
const source = process.env.DELTAMOD_AUDIT_SOURCE_ROOT || path.resolve(__dirname, '../../..');
const { compare } = require(path.join(source, 'scripts/tauri-parity/compare-contract.js'));
const channels = [{ name: 'startGame', classification: 'implemented' }];
function sample() {
  return { schemaVersion: 1, cases: [{ id: 'start-game', channel: 'startGame', data: [], legacy: { ok: true, value: true }, rust: { ok: true, value: true }, expectEquivalent: true, classification: 'implemented' }] };
}
function output() { return { 'start-game': { ok: true, value: true } }; }
test('accepts complete fixtures and JSON object keys in a different order', () => {
  assert.deepEqual(compare(sample(), output(), { 'start-game': { value: true, ok: true } }, channels), []);
});
test('rejects empty, future-schema and malformed fixtures rather than passing or throwing', () => {
  for (const fixture of [null, [], {}, { schemaVersion: 2, cases: [] }, { schemaVersion: 1, cases: [] }, { schemaVersion: 1, cases: [null] }]) {
    const errors = compare(fixture, {}, {}, channels);
    assert.ok(errors.length);
  }
});
test('rejects missing output and missing expectations', () => {
  assert.ok(compare(sample(), {}, {}, channels).length);
  const fixture = sample();
  delete fixture.cases[0].legacy; delete fixture.cases[0].rust;
  assert.ok(compare(fixture, {}, {}, channels).length);
});
test('rejects duplicate cases and unexpected outputs', () => {
  const fixture = sample(); fixture.cases.push({ ...fixture.cases[0] });
  assert.ok(compare(fixture, output(), output(), channels).some(x => /duplicate/.test(x)));
  assert.ok(compare(sample(), { ...output(), orphan: {} }, output(), channels).some(x => /unexpected/.test(x)));
});
test('does not accept inherited output entries', () => {
  assert.ok(compare(sample(), Object.create(output()), output(), channels).some(x => /own output/.test(x)));
});
test('detects classification drift even when both static snapshots match', () => {
  const fixture = sample(); fixture.cases[0].classification = 'unsupported';
  fixture.cases[0].rust = { ok: false, error: 'TAURI_COMMAND_UNAVAILABLE:startGame' };
  fixture.cases[0].expectEquivalent = false;
  assert.ok(compare(fixture, output(), { 'start-game': fixture.cases[0].rust }, channels).some(x => /stale classification/.test(x)));
});
test('implemented classification cannot conceal an unavailable response', () => {
  const fixture = sample(); fixture.cases[0].rust = { ok: false, error: 'TAURI_COMMAND_UNAVAILABLE:startGame' };
  fixture.cases[0].expectEquivalent = false;
  assert.ok(compare(fixture, output(), { 'start-game': fixture.cases[0].rust }, channels).some(x => /stale unavailable/.test(x)));
});
test('unknown channels remain explicitly unknown, not implemented', () => {
  const fixture = sample(); fixture.cases[0].channel = 'shell:open'; fixture.cases[0].classification = 'unknown';
  fixture.cases[0].rust = { ok: false, error: 'TAURI_COMMAND_UNAVAILABLE:unknown' };
  fixture.cases[0].expectEquivalent = false;
  const rust = { 'start-game': fixture.cases[0].rust };
  assert.deepEqual(compare(fixture, output(), rust, channels), []);
  fixture.cases[0].classification = 'implemented';
  assert.ok(compare(fixture, output(), output(), channels).length);
});
test('committed static fixture agrees with current known classifications', () => {
  const base = path.join(source, 'scripts/tauri-parity/fixtures');
  const fixture = require(path.join(base, 'contract.json'));
  const legacy = require(path.join(base, 'legacy-output.json'));
  const rust = require(path.join(base, 'rust-output.json'));
  assert.deepEqual(compare(fixture, legacy, rust, [...channels, { name: 'version', classification: 'implemented' }]), []);
});
test('deterministic malformed-fixture fuzz: 10000 cases fail closed without crashes', () => {
  let state = 0xa17d17;
  const next = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state; };
  for (let i = 0; i < 10000; i++) {
    const fixture = sample();
    const legacy = output();
    const rust = output();
    const value = `${i}-${next().toString(36)}`;
    // Mutate different schema/contract boundaries, rather than repeating a
    // tiny constant corpus thousands of times and calling it new coverage.
    switch ((next() >>> 16) % 12) {
      case 0: fixture.schemaVersion = i + 2; break;
      case 1: fixture.cases[0].id = `${value}/bad`; break;
      case 2: fixture.cases[0].channel = { invalid: value }; break;
      case 3: fixture.cases[0].data = value; break;
      case 4: delete fixture.cases[0].legacy; fixture.cases[0].id = value; break;
      case 5: delete fixture.cases[0].rust; fixture.cases[0].id = value; break;
      case 6: fixture.cases[0].expectEquivalent = value; break;
      case 7: fixture.cases.push({ ...fixture.cases[0], data: [value] }); break;
      case 8: delete legacy['start-game']; legacy[value] = {}; break;
      case 9: rust['start-game'].value = value; break;
      case 10: fixture.cases[0].classification = value; break;
      case 11: fixture.cases[0] = [value]; break;
    }
    assert.ok(compare(fixture, legacy, rust, channels).length, `mutation ${i}`);
  }
});

test('unavailable classifications cannot claim successful mocked responses', () => {
  for (const classification of ['unsupported', 'unknown']) {
    const fixture = sample(); fixture.cases[0].classification = classification;
    const backend = classification === 'unknown' ? [] : [{ name: 'startGame', classification }];
    assert.ok(compare(fixture, output(), output(), backend).some(message => /must not claim success/.test(message)));
  }
});
