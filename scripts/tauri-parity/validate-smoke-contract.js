#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

const REQUIRED_AREAS = new Set([
  'launch', 'mods', 'themes', 'install', 'patch', 'game', 'network', 'updater'
]);

function validateSmokeContract(contract) {
  const errors = [];
  if (contract?.schemaVersion !== 1) errors.push('schemaVersion must be 1');
  if (!contract?.name || typeof contract.name !== 'string') errors.push('name must be non-empty');
  const modes = new Set(contract?.requiredModes || []);
  for (const mode of ['dev', 'packaged']) {
    if (!modes.has(mode)) errors.push(`requiredModes must include ${mode}`);
  }
  if (!contract?.executable?.argument || !contract?.executable?.environmentVariable) {
    errors.push('executable must define an argument and environment variable');
  }
  if (contract?.executable?.requiredForPackagedMode !== true) {
    errors.push('packaged mode must require an explicit executable');
  }
  const isolation = contract?.isolation;
  if (!isolation?.dataRootArgument || !isolation?.dataRootEnvironmentVariable) {
    errors.push('isolation must define a disposable data-root input');
  }
  for (const key of ['disposableOnly', 'fixtureGameRequired', 'realInstallationForbidden']) {
    if (isolation?.[key] !== true) errors.push(`isolation.${key} must be true`);
  }
  const capture = new Set(contract?.capture || []);
  for (const field of ['invoke', 'arguments', 'result', 'error', 'events', 'uiState']) {
    if (!capture.has(field)) errors.push(`capture must include ${field}`);
  }
  if (!Array.isArray(contract?.steps) || contract.steps.length === 0) {
    errors.push('steps must be a non-empty array');
    return errors;
  }
  const ids = new Set();
  const areas = new Set();
  for (const [index, step] of contract.steps.entries()) {
    const prefix = `steps[${index}]`;
    if (!step?.id || typeof step.id !== 'string') errors.push(`${prefix}.id must be non-empty`);
    else if (ids.has(step.id)) errors.push(`${prefix}.id is duplicated: ${step.id}`);
    else ids.add(step.id);
    if (!REQUIRED_AREAS.has(step?.area)) errors.push(`${prefix}.area is unknown: ${step?.area}`);
    else areas.add(step.area);
    if (!step?.channel || typeof step.channel !== 'string') errors.push(`${prefix}.channel must be non-empty`);
    if (!Array.isArray(step?.data)) errors.push(`${prefix}.data must be an array`);
    for (const field of ['result', 'event', 'ui']) {
      if (!step?.expected?.[field] || typeof step.expected[field] !== 'string') {
        errors.push(`${prefix}.expected.${field} must be non-empty`);
      }
    }
  }
  for (const area of REQUIRED_AREAS) {
    if (!areas.has(area)) errors.push(`steps must cover area ${area}`);
  }
  return errors;
}

function load(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

if (require.main === module) {
  const fixture = path.resolve(process.argv[2] || path.join(__dirname, 'fixtures', 'packaged-smoke.json'));
  const errors = validateSmokeContract(load(fixture));
  if (errors.length) {
    console.error(errors.join('\n'));
    process.exit(1);
  }
  console.log(`smoke contract ok: ${fixture}`);
}

module.exports = { REQUIRED_AREAS, validateSmokeContract };
