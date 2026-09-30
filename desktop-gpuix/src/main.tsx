// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { writeFileSync } from 'node:fs';
import { render } from '@gpuix/react';
import { App } from './app.js';
import { AppModel, shortcutFor } from './model.mjs';
import { startBridge } from './bridge.mjs';
import { parseOptions } from './options.mjs';
import { DesktopFeatures } from './features.mjs';
import { ManagedRuntime } from './managed.mjs';
import { UpdateRuntime } from './updater.mjs';

const options = parseOptions(process.argv.slice(2));
const bridge = startBridge(options);
const model = new AppModel(bridge);
const features = new DesktopFeatures(model, bridge);
const managed = new ManagedRuntime(bridge);
const updater = new UpdateRuntime();
let marked = false;
const close = () => { updater.dispose(); managed.dispose(); features.dispose(); model.dispose(); };
process.once('exit', close);
process.once('SIGINT', () => { close(); process.exit(130); });
process.once('SIGTERM', () => { close(); process.exit(143); });

try {
  const snapshot = await model.initialize();
  await features.initialize();
  await managed.initialize();
  if (options.openLink) features.reviewLink(options.openLink);
  const opaque = options.opaque ?? snapshot.preferences.opaque;
  render(<App model={model} features={features} managed={managed} updater={updater} overrides={options} onCommitted={() => {
    if (marked || !options.benchmarkFile) return;
    marked = true;
    // This is a model/React-commit handshake, NOT a native paint measurement.
    // The external benchmark additionally requires painted bounds + a GPU image.
    writeFileSync(options.benchmarkFile, JSON.stringify({
      schemaVersion: 1, phase: 'model-and-react-commit', runtime: 'gpuix-react',
      pid: process.pid, hostPid: bridge.child.pid,
      sourceAttached: model.state.snapshot?.sourceAttached === true,
    }) + '\n', { flag: 'wx' });
  }} />, {
    title: 'Deltamod Community · GPUIX preview', appName: 'Deltamod GPUIX preview',
    width: 1160, height: 780, focus: options.focus,
    windowBackground: process.platform === 'darwin' && !opaque ? 'blurred' : 'opaque',
    onKeyDown(event) {
      if (event.key === 'f11' && managed.state.controller.active) {
        void managed.controllerStop();
        return;
      }
      const action = shortcutFor(event);
      if (action) model.dispatchShortcut(action);
    },
  });
} catch (error) {
  close();
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
