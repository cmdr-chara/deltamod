# Desktop performance evidence

## September 2026 batch

The production boot entry is now `web/boot-native-entry.js`. It uses DOM and a
32-by-32 canvas, not React. The existing React components remain available for
standalone previews but are not part of the production bundle. The native boot
preserves the pixel-heart geometry, theme colors, progress, reduced motion and
explicit video cues. Its assembly animation is deliberately simpler. It temporarily
shares the existing theme video node instead of creating a second decoder, restores
that node on dismissal, stops its idle frame loop and releases its callbacks.

The startup loader fetches classic core scripts concurrently with `async=false`,
preserving their execution order. Linux compatibility scripts load only on Linux.
The updater notice loads after core scripts and retrieves current native status,
so a deferred mount does not lose an already-started update. A failed startup is
not retried over a partially initialized renderer. Existing route-readiness checks,
updater promises, navigation decisions and backend command results are unchanged.

`npm run build:boot` also stages `dist/frontend/`. Tauri embeds that directory rather
than the entire source `web/` tree. Development components, source maps and source
entries are excluded. Theme audio/video served through the validated asset adapter
is kept only in the existing external `themes/` resource. Direct media references
are conservatively retained in the embedded tree for compatibility. Images and
JSON are retained too. Source/browser previews keep their original URLs.

No theme, soundtrack, saved game, recovery copy or native tool is deleted. The
complete G3MTool, UndertaleModTool and Butler resources still ship with the app.
On-demand native tools were considered but are not enabled: the current release
gate requires complete licensed tool trees and offline availability. That migration
needs a separately verified install/update/rollback path, not merely a packaging
exclusion. Existing source archives, signatures and license checks remain mandatory.

## What is measured

The paired Windows workflow builds exact baseline `c2312c720266460454f9f156ccd0f3911162dee6`
and candidate revisions using their committed dependency locks on one runner. It
installs each unsigned package, snapshots the complete installation, then runs both
snapshots serially with independent warmups and seven measurements each. Measured
order alternates AB/BA. Each launch receives a fresh profile and the same bounded
synthetic DELTARUNE seed. No measured launch is discarded as an outlier.

The workflow is available as `Paired Windows performance evidence`. It runs on the
isolated benchmark PR #121 and can also be dispatched manually. Ordinary performance
PRs do not automatically run this expensive lane. It does not publish a release.

Artifacts include:

- `baseline.json`, `candidate.json` and `comparison.json`: authenticated route
  readiness and process-tree working-set measurements, with complete installed
  directory sizes rather than installer sizes.
- `pair-diagnostics.json`: launch order, actual memory-query timing and bounded
  candidate startup traces. `pair-error.json` records an incomplete run instead of
  pretending that a partial capture is a successful comparison.
- Separate package footprint reports for installed bytes, installer download bytes,
  tool/resource categories, largest files and duplicate byte content. The generated
  frontend report distinguishes omitted duplicate media from development sources.

With seven measurements, nearest-rank p95 is the maximum. The shared legacy memory
sampler launches a process query per observation: query overhead can stretch its
nominal two-second window. Actual timing is recorded. These measurements should
not be described as idle memory or a continuously sampled two-second peak.

## Startup outlier investigation

The old 5.51-second launch contains no per-stage trace, so its cause is not proven.
The candidate records at most 96 script/IPC timings, without IPC argument values,
credentials or user paths. Only after a real native benchmark-ready acknowledgement
does it emit a bounded `DELTAMOD_STARTUP` diagnostic. Boot dismissal has a separate
mark and is not substituted for the historical main-route readiness marker.

The normal updater check still returns its actual native result and may involve
network latency. This batch does not fake a successful check or silently bypass it.
Use the slow-operation timings and repeated paired runs to separate application
work from WebView2/VM scheduling and external-service variance.

Until the paired artifacts are available, the changes are implemented optimizations,
not a new proven percentage reduction in whole-app RAM, startup time or package size.
The synthetic fixture also does not prove performance with a large real mod library.

## Focused checks

```console
node --test scripts/desktop-benchmark/performance.test.cjs
npm run build:boot
npm test -- tests/boot-integration.test.js
npm run test:e2e -- tests/e2e/native-boot.spec.js
```

Native installed Windows, Linux and macOS verification remains required for the
external media protocol, CSP and staged frontend path. Browser fixtures do not
establish those package-specific contracts or release approval.
