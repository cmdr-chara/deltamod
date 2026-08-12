#!/usr/bin/env node
const fs = require('node:fs');

function load(file) { return JSON.parse(fs.readFileSync(file, 'utf8')); }
function compare(fixture, legacy, rust) {
  const errors = [];
  for (const test of fixture.cases) {
    const a = legacy[test.id];
    const b = rust[test.id];
    if (JSON.stringify(a) !== JSON.stringify(test.legacy)) errors.push(`${test.id}: legacy output differs from fixture`);
    if (JSON.stringify(b) !== JSON.stringify(test.rust)) errors.push(`${test.id}: Rust output differs from fixture`);
    if (test.expectEquivalent && JSON.stringify(a) !== JSON.stringify(b)) errors.push(`${test.id}: expected legacy/Rust equivalence`);
  }
  return errors;
}
if (require.main === module) {
  const [fixtureFile, legacyFile, rustFile] = process.argv.slice(2);
  if (!fixtureFile || !legacyFile || !rustFile) { console.error('usage: node compare-contract.js fixture.json legacy.json rust.json'); process.exit(2); }
  const errors = compare(load(fixtureFile), load(legacyFile), load(rustFile));
  if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
  console.log(`contract ok: ${load(fixtureFile).cases.length} cases`);
}
module.exports = { compare };
