// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { inspectBackendGraph } from '../scripts/backend-graph.mjs';
function graph(extra = []) {
  const names = ['deltamod-gpuix-host', 'deltamod-app-runtime', ...extra];
  return { packages: names.map(name => ({ id: name, name, version: '1.0.0' })), resolve: { nodes: names.map((name, index) => ({
    id: name, deps: index === 0 ? [{ name: 'old-shell-alias', pkg: names[1], dep_kinds: [{ kind: null }] }] : []
  })) } };
}
test('the shared runtime is accepted under a compatibility import alias', () => {
  assert.equal(inspectBackendGraph(graph()).tauriFree, true);
});
test('a transitively renamed shell dependency is rejected by actual package ID', () => {
  const value = graph(['tauri-runtime-wry']);
  value.resolve.nodes[1].deps.push({ name: 'innocent-alias', pkg: 'tauri-runtime-wry', dep_kinds: [{ kind: null }] });
  assert.throws(() => inspectBackendGraph(value), /deltamod-app-runtime.*tauri-runtime-wry/);
});
test('a build dependency cannot smuggle the Tauri build script into the host', () => {
  const value = graph(['tauri-build']);
  value.resolve.nodes[1].deps.push({ pkg: 'tauri-build', dep_kinds: [{ kind: 'build' }] });
  assert.throws(() => inspectBackendGraph(value), /tauri-build/);
});
test('unused and development-only lock packages do not imply a production dependency', () => {
  const value = graph(['tauri']);
  value.resolve.nodes[1].deps.push({ pkg: 'tauri', dep_kinds: [{ kind: 'dev' }] });
  assert.equal(inspectBackendGraph(value).packages, 2);
});
test('missing resolved nodes are not accepted as a clean graph', () => {
  const value = graph();
  value.resolve.nodes.pop();
  assert.throws(() => inspectBackendGraph(value), /incomplete/);
});
