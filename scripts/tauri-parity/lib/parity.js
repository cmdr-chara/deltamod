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

function blank(value) {
  return value.replace(/[^\r\n]/g, ' ');
}

function stripRustComments(source) {
  let output = '';
  for (let index = 0; index < source.length;) {
    if (source.startsWith('//', index)) {
      const end = source.indexOf('\n', index);
      const stop = end === -1 ? source.length : end;
      output += blank(source.slice(index, stop));
      index = stop;
      continue;
    }
    if (source.startsWith('/*', index)) {
      const start = index;
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source.startsWith('/*', index)) {
          depth += 1;
          index += 2;
        } else if (source.startsWith('*/', index)) {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      output += blank(source.slice(start, index));
      continue;
    }
    if (source[index] === '"') {
      const start = index++;
      while (index < source.length) {
        if (source[index] === '\\') index += 2;
        else if (source[index++] === '"') break;
      }
      output += source.slice(start, index);
      continue;
    }
    const raw = /^(?:b?r)(#{0,16})"/.exec(source.slice(index));
    if (raw) {
      const start = index;
      const terminator = `"${raw[1]}`;
      index += raw[0].length;
      const end = source.indexOf(terminator, index);
      index = end === -1 ? source.length : end + terminator.length;
      output += source.slice(start, index);
      continue;
    }
    output += source[index++];
  }
  return output;
}

function maskRustStrings(source) {
  let output = '';
  for (let index = 0; index < source.length;) {
    if (source[index] === '"') {
      const start = index++;
      while (index < source.length) {
        if (source[index] === '\\') index += 2;
        else if (source[index++] === '"') break;
      }
      output += blank(source.slice(start, index));
      continue;
    }
    const raw = /^(?:b?r)(#{0,16})"/.exec(source.slice(index));
    if (raw) {
      const start = index;
      const terminator = `"${raw[1]}`;
      index += raw[0].length;
      const end = source.indexOf(terminator, index);
      index = end === -1 ? source.length : end + terminator.length;
      output += blank(source.slice(start, index));
      continue;
    }
    output += source[index++];
  }
  return output;
}

function matchingBrace(source, opening) {
  let depth = 0;
  for (let index = opening; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}' && --depth === 0) return index;
  }
  return -1;
}

function stripCfgTestItems(source) {
  let output = source;
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
  let match;
  while ((match = cfgTest.exec(output))) {
    let cursor = match.index + match[0].length;
    while (cursor < output.length && /\s/.test(output[cursor])) cursor += 1;
    while (output.startsWith('#[', cursor)) {
      const attributeEnd = output.indexOf(']', cursor + 2);
      if (attributeEnd === -1) break;
      cursor = attributeEnd + 1;
      while (cursor < output.length && /\s/.test(output[cursor])) cursor += 1;
    }
    const opening = output.indexOf('{', cursor);
    const semicolon = output.indexOf(';', cursor);
    let end;
    if (semicolon !== -1 && (opening === -1 || semicolon < opening)) {
      end = semicolon + 1;
    } else if (opening !== -1) {
      const closing = matchingBrace(maskRustStrings(output), opening);
      end = closing === -1 ? output.length : closing + 1;
    } else {
      end = output.length;
    }
    output = output.slice(0, match.index) + blank(output.slice(match.index, end)) + output.slice(end);
    cfgTest.lastIndex = match.index;
  }
  return output;
}

function rustStringAt(source, start) {
  let cursor = start;
  while (/\s/.test(source[cursor] || '')) cursor += 1;
  if (source[cursor] !== '"') return null;
  let value = '';
  cursor += 1;
  while (cursor < source.length) {
    const character = source[cursor++];
    if (character === '"') return value;
    if (character === '\\') {
      const escaped = source[cursor++];
      const replacements = { n: '\n', r: '\r', t: '\t', '"': '"', '\\': '\\' };
      value += Object.hasOwn(replacements, escaped) ? replacements[escaped] : escaped;
    } else {
      value += character;
    }
  }
  return null;
}

function extractRustFunctions(file, rawSource) {
  const source = stripCfgTestItems(stripRustComments(rawSource));
  const masked = maskRustStrings(source);
  const functions = [];
  const declaration = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\b/g;
  let match;
  while ((match = declaration.exec(masked))) {
    const opening = masked.indexOf('{', declaration.lastIndex);
    const semicolon = masked.indexOf(';', declaration.lastIndex);
    if (opening === -1 || (semicolon !== -1 && semicolon < opening)) continue;
    const closing = matchingBrace(masked, opening);
    if (closing === -1) continue;
    const body = source.slice(opening + 1, closing);
    const bodyMask = masked.slice(opening + 1, closing);
    const events = [];
    const emit = /\.\s*emit\s*\(/g;
    let emitMatch;
    while ((emitMatch = emit.exec(bodyMask))) {
      const event = rustStringAt(body, emit.lastIndex);
      if (event) events.push(event);
    }
    const calls = new Set();
    const call = /(?:\b[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*\b([A-Za-z_][A-Za-z0-9_]*)\s*\(/g;
    let callMatch;
    while ((callMatch = call.exec(bodyMask))) calls.add(callMatch[1]);
    functions.push({
      id: `${file}:${match.index}`,
      file,
      name: match[1],
      events,
      calls
    });
    declaration.lastIndex = opening + 1;
  }
  return functions;
}

function classifyReachableEvents(files) {
  const functions = files.flatMap(({ file, source }) => extractRustFunctions(file, source));
  const byName = new Map();
  for (const fn of functions) {
    if (!byName.has(fn.name)) byName.set(fn.name, []);
    byName.get(fn.name).push(fn);
  }
  const queue = [...(byName.get('main') || [])];
  const visited = new Set();
  const evidence = new Map();
  while (queue.length > 0) {
    const fn = queue.shift();
    if (visited.has(fn.id)) continue;
    visited.add(fn.id);
    for (const event of fn.events) {
      if (!evidence.has(event)) evidence.set(event, []);
      evidence.get(event).push({ file: fn.file, function: fn.name });
    }
    for (const called of fn.calls) queue.push(...(byName.get(called) || []));
  }
  return evidence;
}

function readRustSources(directory, repoRoot) {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...readRustSources(absolute, repoRoot));
    if (entry.isFile() && entry.name.endsWith('.rs')) {
      files.push({
        file: path.relative(repoRoot, absolute),
        source: fs.readFileSync(absolute, 'utf8')
      });
    }
  }
  return files;
}

const REQUIRED_EVENT_PRODUCERS = Object.freeze({
  'leave-controller-mode': path.join('src-tauri', 'src', 'controller.rs'),
  'protocol-download-progress': path.join('src-tauri', 'src', 'channels', 'import_download.rs')
});

function eventProducerReport(files) {
  const evidence = classifyReachableEvents(files);
  const required = Object.entries(REQUIRED_EVENT_PRODUCERS).map(([event, expectedFile]) => {
    const producers = evidence.get(event) || [];
    return {
      event,
      expectedFile,
      producers,
      present: producers.some(producer => producer.file === expectedFile)
    };
  });
  return {
    required,
    missing: required.filter(item => !item.present)
  };
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

function buildParity({ preloadPath, rustPath, rustSourceRoot, rustSources }) {
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
  const sourceRoot = rustSourceRoot || path.dirname(rustPath);
  const repoRoot = path.resolve(sourceRoot, '..', '..');
  const producerReport = eventProducerReport(rustSources || readRustSources(sourceRoot, repoRoot));
  return {
    schemaVersion: 1,
    sources: { preload: path.resolve(preloadPath), rust: path.resolve(rustPath) },
    counts: { electronInvoke: invokes.length, electronEvents: events.length, rustKnown: publicBackend.length, rustImplemented: implemented.length, rustUnsupported: unsupported.length },
    electron: { invokes, events },
    rust: {
      channels: backend,
      publicChannels: publicBackend,
      eventProducers: producerReport.required
    },
    excludedInternal: backend.filter(x => INTERNAL_TAURI_CHANNELS.has(x.name)),
    groups,
    gaps: {
      missingFromRust: missing,
      rustOnly,
      fakeSuccess,
      missingEventProducers: producerReport.missing
    },
    ok: missing.length === 0
      && rustOnly.length === 0
      && fakeSuccess.length === 0
      && producerReport.missing.length === 0
  };
}

function assertParity(report) {
  const errors = [];
  if (report.gaps.missingFromRust.length) errors.push(`missing from Rust: ${report.gaps.missingFromRust.map(x => x.name).join(', ')}`);
  if (report.gaps.rustOnly.length) errors.push(`Rust-only channels: ${report.gaps.rustOnly.map(x => x.name).join(', ')}`);
  if (report.gaps.fakeSuccess.length) errors.push(`fake-success Unsupported channels: ${report.gaps.fakeSuccess.map(x => x.name).join(', ')}`);
  if (report.gaps.missingEventProducers.length) {
    errors.push(`missing Rust event producers: ${report.gaps.missingEventProducers.map(x => `${x.event} in ${x.expectedFile}`).join(', ')}`);
  }
  if (errors.length) throw new Error(errors.join('\n'));
}

module.exports = {
  REQUIRED_EVENT_PRODUCERS,
  extractSet,
  extractRustChannels,
  extractRustFunctions,
  classifyReachableEvents,
  readRustSources,
  eventProducerReport,
  buildParity,
  assertParity,
  classify
};
