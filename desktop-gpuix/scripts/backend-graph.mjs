// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const forbidden = name => /^(?:tauri(?:-|$)|wry$|webkit2gtk(?:-|$)|webview2-com(?:-|$)|deltamod-tauri-shell$)/.test(name);

/** Inspect Cargo's resolved normal/build graph, not Cargo aliases or unused
 * lockfile entries. Dev dependencies are not part of the packaged host. */
export function inspectBackendGraph(metadata) {
  if (!metadata || !Array.isArray(metadata.packages) || !Array.isArray(metadata.resolve?.nodes)) throw new Error('Cargo did not return a resolved dependency graph.');
  const packages = new Map(metadata.packages.map(item => [item.id, item]));
  const nodes = new Map(metadata.resolve.nodes.map(item => [item.id, item]));
  const roots = metadata.packages.filter(item => item.name === 'deltamod-gpuix-host');
  if (roots.length !== 1 || !nodes.has(roots[0].id)) throw new Error('The GPUIX host is missing or ambiguous in the Cargo graph.');
  const pending = [[roots[0].id, []]];
  const visited = new Set();
  const failures = [];
  while (pending.length) {
    const [id, trail] = pending.pop();
    if (visited.has(id)) continue;
    visited.add(id);
    const item = packages.get(id);
    const node = nodes.get(id);
    if (!item || !node || !Array.isArray(node.deps)) throw new Error('Cargo returned an incomplete dependency graph.');
    const chain = [...trail, `${item.name}@${item.version}`];
    if (forbidden(item.name)) failures.push(chain.join(' -> '));
    for (const dependency of node.deps) {
      if (!Array.isArray(dependency.dep_kinds) || !dependency.dep_kinds.length) throw new Error('Cargo omitted dependency kinds.');
      if (dependency.dep_kinds.some(kind => kind.kind !== 'dev')) pending.push([dependency.pkg, chain]);
    }
  }
  if (failures.length) throw new Error('The GPUIX backend still depends on a shell/WebView:\n' + failures.join('\n'));
  if (![...visited].some(id => packages.get(id).name === 'deltamod-app-runtime')) throw new Error('The shared application runtime is not in the GPUIX build.');
  return Object.freeze({ root: roots[0].id, packages: visited.size, tauriFree: true });
}
