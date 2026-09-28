# Recovery, persistence and updater hardening

Base: `ffb3aeb08ab63beab965b55ae8509ac90ea729e4` on
`astra/tauri-only-hardening`. This is source hardening, not a stable-release approval.

## Recovery and filesystem ownership

The profile/install adapter now takes a kernel-backed, nonblocking lease before
filesystem mutations and startup recovery. Clones and independently opened adapters
using the same root cannot perform competing filesystem operations. The persistent
lock file is never unlinked. This does not replace the separate lifecycle engine's
per-installation contract or serialize unrelated applications.

Recovery validates every journal and constructs its action plan before its first
cleanup. Version, operation identity, filename, kind and exact generated paths must
agree. Unknown fields, duplicate fields, newer versions, overlapping destinations,
links, hardlinks and reparse-point ancestors fail closed. Journal reads are limited
to 64 KiB and directory enumeration to 1024 entries. Operation IDs persist across
restarts and remain within the JavaScript safe-integer range.

Only adapter-generated installation, staging, replacement, backup and quarantine
paths are eligible for mutation. Unix cleanup and moves use no-follow directory
handles and no-replace rename. Windows uses pinned ordinary parents and the existing
`fence-windows` mutation API. Cleanup does not recurse through links. Recursive
cleanup has depth and processed-entry budgets. Windows' pinned dependency currently
materializes directory listings before those entry budgets are applied.

Reimport persists installation metadata before removing the previous copy. Failed
publication or restoration retains the journal and recovery copies. Delete moves an
owned installation into quarantine before changing the registry, and restores it
when the registry write fails. Linked external installations point to their actual
source, can be copied into a managed installation, and cannot be repaired or
reimported in place. Unregistering them does not remove their source files.

A sole backup is restored when the live directory is absent. **When both copies
exist, a v1 journal cannot prove which is authoritative. Neither is deleted.** This
condition still requires explicit recovery intervention; no automatic choice, new
recovery UI or game-data erasure is introduced. A malformed or ambiguous journal can
block adapter startup. This conservative behavior is intentional, not a claim of
complete unattended recovery.

Started filesystem operations reach a terminal in-memory state and failure event
even when an error must leave the recovery journal intact.

## Shared persistence

The legacy `##` suffix is recognized only after a complete JSON value. Names and
keys containing the marker are preserved. Other trailing data is rejected. Stored
JSON reads reject non-ordinary files and records larger than 16 MiB before body
allocation. A missing record continues to return the ordinary I/O NotFound error.

Backup publication stages a fresh, independently owned file and renames it instead
of copying into the live backup pathname. Existing target/backup aliases are rejected.
Failed backup publication retains the prior target and backup. A temporary-name
collision never grants cleanup ownership of the colliding file. Unix state and
backup files are created with mode 0600. Parent directories remain caller-owned
trust boundaries; this is not a universal adversarial filesystem sandbox.

## Signed, cancellable update downloads

The pinned updater plugin's progress callback cannot reject a chunk. Its metadata
check and platform installer are retained, but native transport now stops during
reception on budget failure, cancellation, HTTP error, truncation or timeout.

- 512 MiB default artifact budget, checked against headers and every chunk before
  accepting bytes into the artifact buffer. Allocation grows from received bytes,
  not an untrusted declared length.
- 15-second connection, 30-second stalled-read and 10-minute total request limits.
- The initial HTTPS URL is restricted to this repository's release downloads.
  Redirects are bounded and restricted to approved GitHub release hosts. Encoded
  paths, credentials, unexpected ports and transparent decompression are rejected.
- Publisher verification uses the already-locked `minisign-verify` 0.2.5 and the
  immutable application publisher key. Both supported signature formats, key ID,
  authenticated comment and payload are checked. No new external package versions
  or relaxed signature checks are introduced.
- Only an opaque verified payload reaches the plugin's installer. No Node/Electron
  runtime is involved.

`cancel-update` is independent of the updater mutex. Status queries remain responsive
while the download runs. An atomic boundary makes successful cancellation and entry
to installation mutually exclusive. Closing the application requests cancellation
of an active download. Cancellation is polled while awaiting headers or body chunks;
synchronous signature verification completes before its next cancellation check.

The renderer shows accessible progress, cancellation, installation and terminal
states, handles late acknowledgements and unsubscribes on disposal. Installation
errors no longer trigger the initial offer's rejection handler. Release-note markdown
is no longer passed as a constrained single-line update title.

Linux `.deb` automatic updates and macOS ARM CSX remain subject to their existing
explicit capability restrictions. This work does not enable unsupported packaging
or tool combinations.

## Verification and evidence

Local Linux x64 evidence before publication:

- 98 focused Rust unit tests passed across profile/install, shared storage, tools
  and updater/launch. The earlier 12 tool-process tests also passed.
- Strict all-target Clippy passed for these four crates, including the updater's
  native transport feature. Full Tauri workspace formatting and TypeScript passed.
- 10,000 additional generated JSON cases preserve legacy markers in string data.
  The existing 10,000 VDF and 20,000 JavaScript generated-input tests are retained.
  These are deterministic cases, not coverage-guided fuzzing.
- Loopback HTTP tests cover signed success, chunk/header limits, stalled cancellation,
  rejected progress, invalid signatures, truncation, encoding and HTTP errors.
  Signature fixtures are independently generated test keys, not release credentials.
- `npm test` passed 459 Vitest tests in 62 files plus 36 Node audit tests and
  secure-updater checks. Renderer tests cover cancellation races, unknown totals,
  errors and disposal.
  IPC evidence now has 130 invokes, 124 implemented classifications and six explicit
  rejections. Static contract evidence is not installed-application evidence.
- The previous Windows-only Steam test failure is fixed by using host-absolute fixture
  paths without weakening injected Linux discovery semantics.
- CI now builds and tests the full shell on Intel macOS as well as Apple Silicon,
  Windows and Linux. The redundant Intel-only tool job is removed; the full job
  retains that tool and CSX coverage. Candidate CI results must be read separately.

Local GTK/WebKitGTK development packages are absent, so full native shell compilation
and real Windows/macOS behavior are delegated to CI rather than claimed locally.
Real installed-package, Steam-client, signing/notarization, updater installation,
upgrade/data-preservation and rollback acceptance remain release requirements.

### Focused tooling benchmark

[`bounded-json-20260928.json`](../benchmarks/tooling/bounded-json-20260928.json)
records all samples and the reproduction command. On a warm 32 MiB sparse invalid
JSON file, median rejection was **30.630 ms** for the former full-file-read approach
and **0.042 ms** for the metadata-bounded reader (10 alternating trials after warmup,
debug Linux x64 build). The same input is rejected in both cases. This is neither a
normal-file throughput measurement nor a desktop startup/memory benchmark.
Historical desktop benchmark evidence is unchanged.

No itch.io integration, upstream Deltamod implementation copying, external dependency
upgrade, stable release, automatic merge or removal of signing gates is included.
