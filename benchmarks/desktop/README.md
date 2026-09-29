# Desktop runtime benchmark

This benchmark compares the last production Electron baseline with the eventual
production Tauri/Rust build. It is a release gate, not a synthetic claim: post-rewrite
results stay empty until the packaged Tauri application can run the same protocol.

## Comparable protocol

- Run on the same Windows host and power profile.
- Build production artifacts from a clean worktree with pinned dependencies.
- Warm the operating-system file cache with one unreported launch.
- Collect seven measured launches, each with a fresh application profile.
- Give every launch a fresh Deltamod data root and WebView2 user-data directory;
  copy only the declared bounded fixture into the data root before launch.
- Start the timer immediately before process launch.
- Mark readiness only after the first window exists, `window.pageN === "main"`, and
  the route-pending guard has been removed.
- The packaged shell writes `.deltamod-benchmark-ready` inside that launch's data
  root only after the renderer reports both conditions. Process liveness alone is
  never accepted by the benchmark harness.
- During the two seconds after readiness, sample every 100 ms and sum the working
  sets of every process belonging to the application.
- Report every sample plus median and nearest-rank p95; do not compare best runs.
- Record unpacked artifact size and the largest packaged files separately.

The Tauri run must preserve the user-visible readiness condition. If the bridge or
renderer changes, the harness may change, but the start point, readiness meaning,
sample count, warm-up policy, fresh-profile policy, and memory window may not.

## Baseline

[`electron-9e6f8af.json`](electron-9e6f8af.json) records the clean Electron baseline.
Commit `9e6f8af` contains local commit `a882423` in its ancestry.

The unpacked Electron artifact currently includes 408 paths under `native/target`.
That makes packaged size a truthful baseline but also identifies a bounded packaging
opportunity. Excluding build-output trees is intentionally not mixed into this
measurement slice; it requires its own approved change and a complete rebuild.

## Packaged Tauri candidate

[`tauri-packaged-windows-x64-20260901-2.0.18.json`](tauri-packaged-windows-x64-20260901-2.0.18.json)
records the unsigned Windows NSIS candidate produced on 2026-09-01 from revision
`5b9681d`. It uses the same bounded fixture and renderer-authenticated readiness
condition as the Electron baseline: one warm-up plus seven measured launches, each
with a fresh Deltamod data root and WebView2 profile. The recorded artifact is the
complete 2.0.18 NSIS installer, not the standalone shell executable.

The recorded comparison reports a 1.20% higher median readiness time and a 42.15%
lower median peak working set. The old 64.34% storage reduction claim was not an
apples-to-apples comparison: Electron recorded an 881.60 MiB unpacked directory,
while Tauri recorded a 314.37 MiB compressed NSIS installer under `unpackedBytes`.
Those historical JSON records remain unchanged. The comparator now returns
`unpackedArtifactBytes: null` with the incompatible artifact kinds instead of a
misleading storage percentage. Measure complete installed directories (including
sidecars, tools and resources) on both sides before claiming installed-size savings.

These are candidate measurements, not release approval: signing, updater, protocol,
install / uninstall, and non-Windows release gates remain independently mandatory.

Reproduce the comparison with:

```text
node scripts/desktop-benchmark/compare.js benchmarks/desktop/electron-9e6f8af.json benchmarks/desktop/tauri-packaged-windows-x64-20260901-2.0.18.json
```

The comparator fails closed if the runtime, hardware identity, launch count, readiness
condition, warm-up policy, profile policy, or memory sampling protocol differs.

## Tauri-to-Tauri performance work

The comparator also accepts a Tauri baseline, so optimizations can be compared with
an earlier Tauri build without relabelling it as Electron. Both runtime directions
retain the same protocol and hardware checks and report median plus nearest-rank
p95 for readiness and process-tree working set. Missing protocol or hardware fields
are not treated as evidence of a match.

The 2026-09-29 source changes target these costs:

- Provider catalogue metadata has its own 64 MiB / 512-entry budget, instead of
  inheriting the 5 GiB archive-cache allowance. Reopening trims existing catalogue
  entries to this budget. Archives, saved games and recovery generations are not
  part of this cleanup. Fewer catalogue pages may remain available offline.
- Cache usage and eviction scan file metadata, not every JSON response. Writes
  borrow the response rather than deep-cloning it. Repeated cache hits update
  approximate LRU at most once per minute per entry, without extending freshness.
  Reads are bounded even if a cache file grows after its metadata is checked.
- The production boot overlay no longer imposes a blanket 5.2-second cinematic
  minimum. Explicit theme/video cues and the completion transition remain intact.
  Repeated completion callbacks and late failure notifications cannot remount an
  already-dismissed overlay.

These are implementation changes, not a new desktop benchmark result. The historical
route-readiness marker does not necessarily mean the boot overlay has disappeared.
Keep that protocol intact for historical comparisons and measure overlay dismissal
separately when assessing the user's wait. No readiness marker was moved earlier.

Before promotion, run the Rust cache tests, renderer checks and the packaged
seven-launch protocol on the same Windows host. Record the complete installed
footprint and the actual installer download separately. Linux renderer fixtures do
not establish Windows WebView2 working-set savings or macOS package behavior.
