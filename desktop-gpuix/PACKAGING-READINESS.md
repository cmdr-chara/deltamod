# GPUIX packaging and native media readiness

This is a preview packaging path, not a signed release or permission to retire
Tauri. `DELTAMOD_GPUIX_CUTOVER=1` is rejected. The production protocol, archive
association, updater feed and rollback path remain owned by Tauri.

## Dependency inputs

From a network-enabled development checkout with Node 22+, Rust 1.89+ and the
platform build prerequisites, run `npm run lock:resolve` in `desktop-gpuix`.
Review and commit **both** `desktop-gpuix/package-lock.json` and
`desktop-gpuix/native/Cargo.lock`. The resolver protects the existing root,
Tauri and native workspace locks against modification. They are not substitutes
for the standalone host lock. The host workspace explicitly carries the existing
vendored GLib soundness patch because Cargo dependency workspaces do not propagate
patches to the workspace doing the resolution.

The two standalone locks were **not generated in the implementation environment**:
Cargo was absent (`spawnSync cargo ENOENT`) and direct registry/repository access
was unavailable. No hand-written dependency graph or checksum is presented as a
resolved lock. `native`, `test:native`, staging and packaging now reject unlocked
inputs. On a capable checkout, follow lock generation with `npm ci`, the focused
checks, the locked native build, and the existing target-specific sidecar staging.

## Stage real target inputs

Build the frontend and host, then run `npm run stage:host` and
`npm run stage:runtime`. The latter checks native/react 0.10.0, resolves exactly one
matching installed addon, verifies machine headers and stages the addon, npm
production license files and lock provenance. The compiled executable alone is
not a substitute for the dynamically loaded `.node` addon.

`native/runtime.json` records target, app version, lock digests, frontend/host
hashes and the addon hash. Package generation rejects stale binaries or locks.
Startup validates the packaged host/addon and sets napi-rs' exact native-library
path before importing GPUIX. A WASI binding is rejected. Hashes establish byte
consistency with these staged inputs, **not publisher identity or ABI readiness**.

Reviewed package matrix:

| Target | Format | Self-update now |
| --- | --- | --- |
| Windows x64 | NSIS, per-user | Disabled |
| macOS arm64 | `.app` | Disabled |
| Linux x64/glibc | DEB, or explicit AppImage | Disabled |

Select Linux AppImage with `DELTAMOD_GPUIX_FORMAT=appimage`. Intel macOS, ARM Linux
and other unreviewed native addon targets are rejected, not silently relabeled.
Native GPUIX's updater supports AppImage but not DEB. AppImage update/relaunch
rehearsal remains a separate blocker, so neither Linux format is enabled here.

The preview URI is declared only on Windows/Linux. macOS packaging deliberately
omits protocol declarations until real Launch Services open-URL/open-file delivery
exists. CLI handoffs are not evidence that Finder or Safari delivers those events.
Production `.modarchive` and `deltamod-community://` takeover remains disabled on
all platforms. Trusted `.deltamod-open` marker semantics remain unimplemented.

## Native media inputs

Theme preview uses native FFmpeg video decoding to bounded 640x360 BGRA frames
uploaded through GPUIX, and a separate owned FFplay process for optional audio.
There is no WebView, PATH lookup or runtime codec download. Playback is explicit,
muted by default, bounded to one hour, stopped on theme/unmount/preference changes,
and subject to decoder-stall and process-reaping limits. Reduced motion suppresses
video and animated cue transitions without forcing audio on.

Provide a reviewed target-specific tree via `DELTAMOD_GPUIX_MEDIA_DIR`:

```text
native/media.json
native/media/ffmpeg[.exe]
native/media/ffplay[.exe]
native/media/LICENSE
native/media/<required dynamic libraries and notices>
```

`media.json` must contain `schemaVersion: 1`, a matrix `target` such as `linux-x64`,
a canonical HTTPS `source`, `license: "native/media/LICENSE"`, and `ffmpeg`/`ffplay`
objects each containing its relative `path` and the actual lowercase `sha256`.
The stager checks paths, hashes, architecture, licenses, links, file counts and
size limits. It does not download a build or fabricate hashes. Full reviewed
codec provenance, corresponding source/license obligations and dynamic-library
redistribution must be supplied and validated by the release process. Executable
hashes do not validate every dynamically loaded dependency.

For development playback, supply a resource root that includes the reviewed
`native/media.json` tree and the existing `web/themes` assets. Packaged resources
include both. Do not point at arbitrary executable paths or copy unreviewed PATH
binaries to make a readiness check appear green.

The cue uses `bootSyncTime`, seeks both players to the same requested offset and
shows the theme's soul color. The two decoder clocks are **not sample-locked**.
The real decoder smoke only validates frame transport. It does not prove rendered
GPU frames, audible output, long-running A/V synchronization, all bundled codecs,
full background-loop/global sound-effect parity or platform performance.

## Signing, installed validation and updater trust

The only generated channel is `preview` with `updaterRehearsal: "not-performed"`.
The updater refuses to contact a feed or install anything for those packages.
The existing GPUIX-specific feed and publisher verification key are retained and
never redirected to the Tauri feed. Native artifact verification is still
required at download/install time. Discovery metadata is not a verified payload.

Before any future stable path, sign all inner native binaries first, then generate
hashes over the final bytes. Sign the outer package without silently changing the
staged binaries. macOS additionally needs Developer ID, notarization and ticket
validation. Windows needs publisher identity verification and installer signing.
Linux needs clean-host dependency and installation verification. No private keys,
certificates or credentials belong in this repository.

For every target, install on a clean host and exercise a visible native window,
all five existing sidecars, persistence/recovery, archive review/import, cold and
warm handoffs, foreground/minimized behavior, codec startup/stop and uninstall
without removing user data. Rehearse an authenticated update from an earlier
installed version, interruption, signature rejection, restart and rollback.
Only then add a reviewed stable metadata generator and enable a supported target.
Do not manually edit a preview manifest to substitute for this evidence.
