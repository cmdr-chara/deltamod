const fs = require('node:fs');
const path = require('node:path');

function sourceLocation(source, offset) {
  return { line: source.slice(0, offset).split('\n').length };
}

function unquote(value) {
  return value.slice(1, -1).replace(/\\(['"\\])/g, '$1');
}

function extractSet(source, name) {
  const start = new RegExp(`\\b${name}\\s*=\\s*new\\s+Set\\s*\\(\\s*\\[`, 'm').exec(source);
  if (!start) throw new Error(`Could not find JavaScript Set ${name}`);
  const bodyStart = start.index + start[0].length;
  const end = source.indexOf('])', bodyStart);
  if (end < 0) throw new Error(`Unterminated JavaScript Set ${name}`);
  const body = source.slice(bodyStart, end);
  const values = [];
  const literal = /(['"])(?:\\.|(?!\1)[^\\])*\1/g;
  let match;
  while ((match = literal.exec(body))) values.push({
    name: unquote(match[0]),
    ...sourceLocation(source, bodyStart + match.index)
  });
  if (!values.length) throw new Error(`JavaScript Set ${name} is empty or not literal-backed`);
  return values;
}

function extractRustChannels(source) {
  const implStart = source.indexOf('impl FromStr for BackendChannel');
  if (implStart < 0) throw new Error('Could not find BackendChannel FromStr implementation');
  const bodyStart = source.indexOf('fn from_str', implStart);
  const bodyEnd = source.indexOf('\n    }\n}', bodyStart) >= 0
    ? source.indexOf('\n    }\n}', bodyStart)
    : source.indexOf('_ => return Err', bodyStart);
  if (bodyStart < 0 || bodyEnd < 0) throw new Error('Could not bound BackendChannel::from_str');
  const body = source.slice(bodyStart, bodyEnd);
  const channels = new Map();
  const arm = /((?:"(?:\\.|[^"\\])*"\s*(?:\|\s*)?)+)=>\s*Self::(Implemented|Unsupported|[A-Za-z0-9_]+)/g;
  let match;
  while ((match = arm.exec(body))) {
    const names = [...match[1].matchAll(/"(?:\\.|[^"\\])*"/g)];
    for (const item of names) {
      const name = JSON.parse(item[0]);
      const classification = match[2] === 'Unsupported' ? 'unsupported' : 'implemented';
      channels.set(name, {
        name,
        classification,
        ...sourceLocation(source, bodyStart + match.index + item.index)
      });
    }
  }
  if (!channels.size) throw new Error('BackendChannel::from_str yielded no literal channels');
  return [...channels.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function classify(name) {
  if (/^(getMod|howManyMods|toggleMod|setMod|removeMod|importMod|dlmod|precalcGameHashes|modSources:)/.test(name)) return 'mods';
  if (/Theme|theme|Sponsor/.test(name)) return 'themes';
  if (/Install|installation|SystemIndex|Steam|Edition|GameImport/.test(name)) return 'install';
  if (/Game|game|Deltarune|patchAndRun|startGame|loaded/.test(name)) return 'game';
  if (/nexus|gamebanana|Gamebanana|Comment|Like|Collection|Provider|browse/.test(name)) return 'network';
  if (/update|Update|CLI|initialize/.test(name)) return 'updater';
  return 'system';
}

// These are implementation channels used by the Tauri shell, not renderer
// compatibility invokes. They must never be added to preload's public bridge.
const INTERNAL_TAURI_CHANNELS = new Set([
  'protocol:parseDeepLink',
  'protocol:planRange',
  'protocol:queueDeepLink',
  'protocol:rendererReady',
  'modSources:validateUrl'
]);

function buildParity({ preloadPath, rustPath }) {
  const preload = fs.readFileSync(preloadPath, 'utf8');
  const rust = fs.readFileSync(rustPath, 'utf8');
  const invokes = extractSet(preload, 'ALLOWED_INVOKE_CHANNELS');
  const events = extractSet(preload, 'ALLOWED_EVENT_CHANNELS');
  const backend = extractRustChannels(rust);
  const electron = new Set(invokes.map(x => x.name));
  const publicBackend = backend.filter(x => !INTERNAL_TAURI_CHANNELS.has(x.name));
  const rustByName = new Map(publicBackend.map(x => [x.name, x]));
  const missing = invokes.filter(x => !rustByName.has(x.name));
  const rustOnly = publicBackend.filter(x => !electron.has(x.name));
  const fakeSuccess = [];
  const implemented = publicBackend.filter(x => x.classification === 'implemented');
  const unsupported = publicBackend.filter(x => x.classification === 'unsupported');
  const groups = {};
  for (const item of invokes) (groups[classify(item.name)] ||= []).push(item.name);
  return {
    schemaVersion: 1,
    sources: { preload: path.resolve(preloadPath), rust: path.resolve(rustPath) },
    counts: { electronInvoke: invokes.length, electronEvents: events.length, rustKnown: publicBackend.length, rustImplemented: implemented.length, rustUnsupported: unsupported.length },
    electron: { invokes, events },
    rust: { channels: backend, publicChannels: publicBackend },
    excludedInternal: backend.filter(x => INTERNAL_TAURI_CHANNELS.has(x.name)),
    groups,
    gaps: { missingFromRust: missing, rustOnly, fakeSuccess },
    ok: missing.length === 0 && rustOnly.length === 0 && fakeSuccess.length === 0
  };
}

function assertParity(report) {
  const errors = [];
  if (report.gaps.missingFromRust.length) errors.push(`missing from Rust: ${report.gaps.missingFromRust.map(x => x.name).join(', ')}`);
  if (report.gaps.rustOnly.length) errors.push(`Rust-only channels: ${report.gaps.rustOnly.map(x => x.name).join(', ')}`);
  if (report.gaps.fakeSuccess.length) errors.push(`fake-success Unsupported channels: ${report.gaps.fakeSuccess.map(x => x.name).join(', ')}`);
  if (errors.length) throw new Error(errors.join('\n'));
}

module.exports = { extractSet, extractRustChannels, buildParity, assertParity, classify };
