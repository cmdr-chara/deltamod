# Tauri IPC gap inventory

This inventory records renderer-facing Electron event contracts and requires a reachable, non-test Rust producer before an event is counted as implemented. A literal in a comment, a `#[cfg(test)]` item, or an uncalled helper is not production evidence. `tests/tauri-parity-classification.test.js` enforces that rule for the final two gaps.

Current public bridge baseline: 129 invoke channels (123 implemented, 6 explicitly unsupported) and 18 renderer event channels. The final renderer-event gap count is **0**. Command classification is tracked separately and can still make a broader parity gate fail as the migration evolves.

| Renderer event | Tauri production path | Status |
| --- | --- | --- |
| `page`, `audio` | `state::TauriGameLifecycle` and patch lifecycle callbacks | Present |
| `gplog` | patch/file-handoff failure callbacks | Present |
| `updateAvailable`, `updater-status`, `updater-progress` | `state::UpdateEvents::emit` | Present |
| `themeChange`, `refresh` | `main::renderer_event` through `emit_runtime_events`; protocol/local imports also refresh | Present |
| `finishedPatch`, `hash-progress` | `channels::patching` operation callbacks | Present |
| `dlmodURL-progress` | renderer-invoked `channels::import_download::download_mod` callback | Present |
| `protocol-download-progress` | validated OS/startup protocol handoff → serialized renderer-ready worker → `run_protocol_import` → `download_allowlisted` callback | Present (D2g) |
| `profile-import-progress` | `channels::dialogs` profile import event drain | Present |
| `game-import-progress` | game download/import callbacks in `channels::import_download` and `channels::workflows` | Present |
| `winResAlert` | main-window resize coalescer in `main.rs` | Present |
| `leave-controller-mode` | controller-only native `Exit Controller Mode` menu item with F11 accelerator → `on_menu_event` | Present (D2g) |
| `mod-source-progress` | Nexus download/import callback in `channels::nexus_download` | Present |
| `installer-progress` | bounded installer workflow in `main.rs` | Present |

## D2g contract evidence

### `leave-controller-mode`

- Electron source: `node/Runner.js` creates `Exit Controller Mode` with accelerator `F11` only inside the active controller-mode branch.
- Tauri source: `controller::install_controller_exit_menu` creates that native action only when `ControllerMode::enabled()` is true. `controller::handle_controller_menu_event` rechecks managed controller state and the exact menu item ID before emitting; `main` only delegates the menu event.
- Payload: JSON `null`, preserving the renderer's no-argument/null-compatible callback contract.
- Ownership: Rust emits only the request. The existing renderer prompt and `cmode-off` relaunch remain renderer-owned.
- Negative evidence: an ordinary F11 path is not registered outside controller mode; unrelated menu IDs and non-controller state cannot emit.

### `protocol-download-progress`

- Input trust boundary: startup arguments and macOS `RunEvent::Opened` URLs accept the strict query form parsed by `deltamod_protocol_domain::parse_deep_link` and the retained exact path form `deltamod-community://gb/Mod/{item_id}/https://.../mmdl/{file_id}`. The initial GameBanana source path must end at the same decimal `file_id`, while item identity remains the bounded value from the trusted query/path. Arbitrary URLs, malformed/duplicate identities, non-HTTPS, credentialed, IP/localhost, alternate-port, redirect-shaped, query-bearing, and non-GameBanana sources fail closed before network access.
- Readiness/concurrency: validated actions wait for managed application state, `PageLoad::Finished`, and an explicit renderer handshake that confirms all protocol event listeners were successfully registered for the same navigation generation. A new `PageLoad::Started`, stale-generation handshake, renderer unload, window destruction, or app exit cancels that generation. Work remains in a bounded FIFO with one owned worker.
- Network boundary: `download_allowlisted` revalidates HTTPS and the GameBanana host allowlist, caps redirects, and enforces the existing 2 GiB mod limit.
- Producer: its real chunk callback emits exactly `operationId`, `phase`, `completed`, `total`, `currentItem`, and `percentage`. The operation ID is one UUID retained for the operation. Unknown totals use `total: 0` and `percentage: null`, matching Electron. `currentItem` is a bounded identifier-only label and never a remote URL.
- Import/cleanup: the downloaded temporary file remains owned by `DownloadedFile` and is deleted on drop; the existing archive importer validates, stages, atomically commits, and cleans its own staging state. Errors exposed to the renderer are fixed messages without raw URLs or remote error text.
- Separation: this path never emits or aliases `dlmodURL-progress`; that event remains exclusive to renderer-triggered downloads.
- Ownership/cancellation: the app state retains every async worker handle, active imports receive a generation-owned cancellation token, and shutdown cancels the active operation, clears queued work, and aborts retained workers. A dequeued-but-not-started action is requeued only when it is safe to do so; already-running stale work cannot emit into a newer generation.

### Producer parity gate

- The parity report scans reachable, non-test Rust functions from `main.rs` and requires `leave-controller-mode` in `controller.rs` plus `protocol-download-progress` in `channels/import_download.rs`.
- Both the focused Vitest and standalone harness mutate each real source in memory to delete its event literal. Each mutation must make `report.ok` false and `assertParity` throw, so comments, test-only literals, or producers in the wrong file cannot satisfy the gate.

## Packaged protocol registration

The shell now registers `deltamod-community://` through Tauri's desktop deep-link
plugin and installs the single-instance plugin before it. A protocol click aimed
at an already-running process therefore forwards its argument vector to
`controller::protocol_second_instance`; that boundary drops the executable
argument and sends every remaining value through the same strict protocol/local
handoff classifier used at cold start. The renderer producer and bounded queue
described above remain the only import path after OS delivery.

`tests/tauri-protocol-registration.test.js` freezes the manifest scheme, plugin
ordering, and executable-argument removal. Runtime registration still belongs to
the packaged platform smoke matrix because unit tests cannot prove that Windows,
Launch Services, or the Linux desktop database accepted an installed handler.
