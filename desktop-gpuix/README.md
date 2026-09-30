# Deltamod GPUIX preview

Parallel React/GPUIX desktop frontend, with a private stdio connection to Rust.
It starts **no Tauri window and no WebView**. The normal Deltamod app is unchanged.

**Stage 1: read-only prototype. Not a replacement release.** The native build and
GPUI-rendered UI must be validated on an actual desktop before use or performance
claims. No measured speed, RAM or package-size advantage is claimed by this branch.

## Included

| Surface | Implemented behavior |
| --- | --- |
| Shell | Native window, sidebar, compact layout, focus traversal, navigation and dialogs |
| Home | Counts from the attached profile, empty/error states and refresh |
| Mod library | Bounded read-only listing, search and details for runtime and legacy packet records |
| Installations | Existing store records and details, without probing saved game paths |
| Mod Shop | Public DELTARUNE GameBanana browse/search/pagination, error/retry and allowlisted mod-page links |
| Themes | Built-in catalogue palette selection, persisted only in preview preferences |
| Settings | Reduced motion, opaque/translucent surfaces, session-only source-profile attachment |
| Animation | GPUIX native opacity/width tweens, no JavaScript frame timer |
| Backend | Existing storage-domain, mods-themes-domain and network-runtime crates |

Account login, downloads/imports, enable/disable, patching, recovery, game launch,
controller support, custom CSS/video/audio themes, localization and signed updates
are **not ported**. They remain available in Tauri. Unsupported mutations are absent
from the native command allowlist, not simulated as successful operations.

The theme view is a color preview, not full theme parity. macOS can use the
framework's blurred window backdrop after enabling glass and restarting. Windows
and Linux use layered translucent fills. This is not a claim of system Liquid Glass
or a custom refraction shader. Opaque surfaces and reduced motion default on.

## Run from this repository

Requirements: Node.js 22+, Rust, and a desktop supported by GPUIX's native package.
The Rust backend must be built before opening the window.

```sh
cd desktop-gpuix
npm install
npm run native
npm run dev
```

`@gpuix/react` and `@gpuix/native` are pinned together at **0.10.0**. React is real
React, not React-like syntax. No intermediate React DOM version is needed.

This new standalone workspace does not yet have resolved dependency locks. After
resolving dependencies on a connected development machine, review and commit
`package-lock.json` and `native/Cargo.lock`. Use `npm ci` and Cargo `--locked`
thereafter. Neither root Tauri lockfile is modified. Native benchmarking refuses
to run without the new lockfiles.

The native resource root defaults to the repository root, which supplies `games/`
and `web/themes/data/`. No real game is required to open the empty preview.

```sh
npm run dev -- --source-profile /absolute/path/to/deltamod-data
npm run dev -- --no-focus --reduce-motion --opaque
npm run build
npm start
```

`--source-profile` is optional. The native folder picker can attach one later.
Attachment lasts for this process only. The source directory is never passed to a
writable Tauri runtime, copied or migrated. Corrupt records produce errors/warnings,
not an assertion that data is gone. Legacy enabled states which cannot be verified
are displayed as unavailable.

`--state-root`, `--resources-root` and `--backend` allow explicit development paths.
The default state directory is `DeltamodCommunityGPUIX` under the platform's user
application-data directory. The backend refuses a nonempty directory lacking its
own ownership marker, and refuses any source/state directory overlap.

## Validation

```sh
npm test
npm run typecheck
npm run test:native
npm run smoke -- /absolute/path/to/new-evidence-directory
```

The first command tests real transport/model code using a fake child process. It
does not replace Cargo compilation or native GPU testing. The smoke command drives
the actual GPUIX window and writes actual GPU screenshots for all six routes. It
does not assert provider uptime, media playback or mutating-feature parity.

## Experimental measurements

```sh
npm run benchmark -- /absolute/path/to/disposable-fixture /absolute/path/to/new-results
```

Build first and freeze both new lockfiles. The fixture must be a Deltamod data
folder, not a real game folder. It is read-only and shared across launches, while
every preview state directory is fresh. One warmup and seven measurements are
retained. Readiness requires model data, native home bounds and a GPU screenshot.
The harness samples the complete app process tree, including the Rust helper.
Actual query timings are retained alongside the nominal sampling interval.

This readiness includes automation and GPU readback overhead. Its JSON explicitly
marks itself **not comparable to the existing Tauri benchmark protocol**. A smaller
read-only prototype is also not feature-equivalent to the production app. Do not
compare it with the old 396 MiB result as a migration win. A shared workload and
common painted-readiness protocol for both complete frontends is a later gate.

## Migration gates

1. Resolve/freeze dependencies, compile Rust, typecheck the real GPUIX packages and
   inspect native screenshots on Windows, Linux and macOS.
2. Port transactional mod/install/patch workflows through the existing Rust domain
   adapters, with cancellation and recovery verified before enabling their buttons.
3. Complete accounts, localization, accessibility, controller input, real media,
   deep links and trusted packaging/update adapters.
4. Run paired equivalent-workload Tauri/GPUIX measurements on the same host.
5. Retire Tauri only after feature parity, recovery, signed-update and performance
   gates pass. Until then, rollback is simply continuing to use Tauri.

## API references

Implementation checked against `remorses/gpuix` documentation and package source:
- https://gpuix.dev/ (native animations, keyboard navigation, virtual lists and automation)
- https://github.com/remorses/gpuix/tree/main/packages/react

GPUIX is pre-1.0. Its motion API currently supports numeric tweens, not springs,
keyframes or shared layout transitions. Its UI bridge and JavaScript runtime still
have overhead. Raw GPUI/Tauri/GPUIX memory ordering is a hypothesis to measure, not
a guarantee.
