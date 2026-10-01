// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { writeFileSync } from 'node:fs';
import { AppModel, shortcutFor } from './model.mjs';
import { startBridge } from './bridge.mjs';
import { parseOptions } from './options.mjs';
import { DesktopFeatures } from './features.mjs';
import { ManagedRuntime } from './managed.mjs';
import { UpdateRuntime, updateCapability } from './updater.mjs';
import { HandoffInbox, type Handoff } from './handoffs.mjs';
import { startSingleInstance } from './single-instance.mjs';
import { WindowActivation } from './window-lifecycle.mjs';
import { prepareNativeRuntime } from './runtime-layout.mjs';
import { createLauncherReady } from './launcher-ready.mjs';
import { readLaunchMarker, acknowledgeLaunchMarker, abandonLaunchMarker } from './launch-marker.mjs';

const launcherReady = createLauncherReady();
const options = parseOptions(process.argv.slice(2));
// Validate before any native runtime starts. This capability requests a window,
// never an import or arbitrary command. Failed launches retain the CLI marker.
const launchMarker = options.launchMarker ? readLaunchMarker(options.launchMarker) : null;
if (launchMarker) process.once('exit', () => abandonLaunchMarker(launchMarker));
const acknowledgeMarker = () => {
  if (!launchMarker) return;
  try { acknowledgeLaunchMarker(launchMarker); }
  catch { console.error('The CLI launch marker changed before acknowledgement and was retained.'); }
};
const inbox = new HandoffInbox();
const activation = new WindowActivation(message => console.error(message));
const requests: Handoff[] = [];
if (options.openLink) requests.push({ kind: 'preview', value: options.openLink });
if (options.protocolLink) requests.push({ kind: 'protocol', value: options.protocolLink });
if (options.archiveFile) requests.push({ kind: 'archive', value: options.archiveFile });
let starting = true;
const instance = await startSingleInstance(options, requests, items => {
  const receipt = inbox.receive(items);
  // Authenticated secondary launches also foreground an existing window when
  // no file was supplied. First launch still respects --no-focus.
  if (!starting || items.length > 0 || launchMarker) activation.request();
  return receipt;
});
starting = false;
// Secondary launches do not load the addon or start another writable backend.
if (instance.primary) {
  let releaseBackend: (() => void) | undefined;
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    activation.dispose();
    instance.dispose();
    inbox.dispose();
    releaseBackend?.();
  };
  process.once('exit', close);
  process.once('SIGINT', () => { close(); process.exit(130); });
  process.once('SIGTERM', () => { close(); process.exit(143); });
  try {
    // Resolve and verify the packaged addon before any @gpuix import. Bun's
    // executable alone does not contain napi-rs' dynamically located addon.
    const runtimeManifest = prepareNativeRuntime(options);
    const nativeBinding = await import('@gpuix/native');
    if (runtimeManifest && Reflect.get(nativeBinding, '__napiBindingTarget') !== 'native') throw new Error('A native GPUIX binding is required.');
    const { render } = await import('@gpuix/react');
    const { HandoffShell } = await import('./handoff-ui.js');
    const { NativeWindow } = await import('./native-window.js');
    const { ThemeResources } = await import('./media-ui.js');
    const { createElement, Fragment } = await import('react');
    const bridge = startBridge(options);
    releaseBackend = () => { bridge.child.kill(); };
    const model = new AppModel(bridge);
    releaseBackend = () => { model.dispose(); };
    const features = new DesktopFeatures(model, bridge);
    const managed = new ManagedRuntime(bridge);
    const updater = new UpdateRuntime(undefined, updateCapability(runtimeManifest));
    releaseBackend = () => { updater.dispose(); managed.dispose(); features.dispose(); model.dispose(); };
    let marked = false;
    const snapshot = await model.initialize();
    await features.initialize();
    await managed.initialize();
    const opaque = options.opaque ?? snapshot.preferences.opaque;
    render(createElement(ThemeResources.Provider, { value: options.resourcesRoot }, createElement(Fragment, null,
      createElement(NativeWindow, { activation }),
      createElement(HandoffShell, { inbox, model, features, managed, updater, overrides: options, onCommitted: () => {
        launcherReady(options);
        acknowledgeMarker();
        if (marked || !options.benchmarkFile) return;
        marked = true;
        // Model/React-commit handshake, not a native paint measurement.
        writeFileSync(options.benchmarkFile, JSON.stringify({
          schemaVersion: 1, phase: 'model-and-react-commit', runtime: 'gpuix-react',
          pid: process.pid, hostPid: bridge.child.pid,
          sourceAttached: model.state.snapshot?.sourceAttached === true,
        }) + '\n', { flag: 'wx' });
      }})
    )), {
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
} else {
  // A verified receipt proves the existing instance accepted the wake request.
  launcherReady(options);
  acknowledgeMarker();
  activation.dispose();
  inbox.dispose();
}
