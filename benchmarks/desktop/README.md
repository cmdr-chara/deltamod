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

[`tauri-packaged-windows-x64-20260901.json`](tauri-packaged-windows-x64-20260901.json) records the
unsigned Windows NSIS candidate produced on 2026-09-01 after lifecycle retention
and packaged-capability hardening. It uses the same bounded
fixture and renderer-authenticated readiness condition as the Electron baseline:
one warm-up plus seven measured launches, each with a fresh Deltamod data root and
WebView2 profile. The recorded artifact is the complete NSIS installer, not the
standalone shell executable.

The current comparison reports a 9.93% higher median readiness time, a 41.99%
lower median peak working set, and a 66.57% smaller packaged artifact. These are
candidate measurements, not release approval: signing, updater, protocol, install /
uninstall, and non-Windows release gates remain independently mandatory.

Reproduce the comparison with:

```text
node scripts/desktop-benchmark/compare.js benchmarks/desktop/electron-9e6f8af.json benchmarks/desktop/tauri-packaged-windows-x64-20260901.json
```

The comparator fails closed if the runtime, hardware identity, launch count, readiness
condition, warm-up policy, profile policy, or memory sampling protocol differs.
