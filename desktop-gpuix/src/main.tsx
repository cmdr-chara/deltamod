// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { writeFileSync } from 'node:fs';
import { AppModel, shortcutFor } from './model.mjs';
import { startBridge } from './bridge.mjs';
import { parseOptions } from './options.mjs';
import { DesktopFeatures } from './features.mjs';
import { ManagedRuntime } from './managed.mjs';
import { UpdateRuntime } from './updater.mjs';
import { HandoffInbox, type Handoff } from './handoffs.mjs';
import { startSingleInstance } from './single-instance.mjs';

const options = parseOptions(process.argv.slice(2));
const inbox = new HandoffInbox();
const requests: Handoff[] = [];
if (options.openLink) requests.push({ kind: 'preview', value: options.openLink });
if (options.protocolLink) requests.push({ kind: 'protocol', value: options.protocolLink });
if (options.archiveFile) requests.push({ kind: 'archive', value: options.archiveFile });
const instance = await startSingleInstance(options, requests, items => inbox.receive(items));
// Secondary launches return after a bounded acknowledgement. They never create
// another writable Rust runtime or load the native renderer.
if (instance.primary) {
  let releaseBackend: (() => void) | undefined;
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    instance.dispose();
    inbox.dispose();
    releaseBackend?.();
  };
  process.once('exit', close);
  process.once('SIGINT', () => { close(); process.exit(130); });
  process.once('SIGTERM', () => { close(); process.exit(143); });
  try {
    const { render } = await import('@gpuix/react');
    const { HandoffShell } = await import('./handoff-ui.js');
    const { createElement } = await import('react');
    const bridge = startBridge(options);
    releaseBackend = () => { bridge.child.kill(); };
    const model = new AppModel(bridge);
    releaseBackend = () => { model.dispose(); };
    const features = new DesktopFeatures(model, bridge);
    const managed = new ManagedRuntime(bridge);
    const updater = new UpdateRuntime();
    releaseBackend = () => { updater.dispose(); managed.dispose(); features.dispose(); model.dispose(); };
    let marked = false;
    const snapshot = await model.initialize();
    await features.initialize();
    await managed.initialize();
    const opaque = options.opaque ?? snapshot.preferences.opaque;
    render(createElement(HandoffShell, { inbox, model, features, managed, updater, overrides: options, onCommitted: () => {
      if (marked || !options.benchmarkFile) return;
      marked = true;
      // This is a model/React-commit handshake, NOT a native paint measurement.
      // The external benchmark additionally requires painted bounds + a GPU image.
      writeFileSync(options.benchmarkFile, JSON.stringify({
        schemaVersion: 1, phase: 'model-and-react-commit', runtime: 'gpuix-react',
        pid: process.pid, hostPid: bridge.child.pid,
        sourceAttached: model.state.snapshot?.sourceAttached === true,
      }) + '\n', { flag: 'wx' });
    }}), {
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
} else { inbox.dispose(); }
