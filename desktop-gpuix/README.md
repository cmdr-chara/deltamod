# Deltamod GPUIX preview

Parallel React/GPUIX desktop frontend, with a private stdio connection to Rust.
It starts **no Tauri window and no WebView**. The normal Deltamod app is unchanged.

**Stage 1: read-only prototype. Not a replacement release.** The native build and
GPUI-rendered UI must be validated on an actual desktop before use or performance
claims. No measured speed, RAM or package-size advantage is claimed by this branch.

## Included

| Surface | Implemented behavior |
| --- | --- |
| Shell | Native window, compact sidebar, back/forward history, keyboard shortcuts, focus traversal and modal dialogs |
| Home | Counts from the attached profile, empty/error states and refresh |
| Mod library | Profile-wide search, enabled/disabled/unknown filters, format filtering, sorting and details |
| Installations | Session-only preview selection and details, without changing Tauri selection or probing saved game paths |
| Mod Shop | Game-aware GameBanana browse/search/pagination, catalogue game picker, read-only mod details, file metadata and allowlisted links |
| Themes | Built-in palette selection and opt-in still-image previews with bounded local paths |
| Settings | Reduced motion, opaque/translucent surfaces, session-only profile attachment and confirmed disconnection |
| Language | English and Italian interface messages with preserved named placeholders and English fallback |
| Links | Reviewed, confirmed navigation from pasted URLs or the --open command-line argument |
| Animation | GPUIX native opacity/width tweens, no JavaScript frame timer |
| Backend | Existing storage-domain, mods-themes-domain and network-runtime crates |

Account login, downloads/imports, enable/disable, patching, recovery, game launch,
controller support, custom CSS/video/audio themes, additional languages and signed updates
are **not ported**. They remain available in Tauri. Unsupported mutations are absent
from the native command allowlist, not simulated as successful operations.

Theme previews cover colors and still images, not full theme parity. macOS can use the
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

## Read-only workspace controls

Select an installation on **Installations** to choose the preview's game context.
**Browse mods** selects it and opens its GameBanana catalogue. The selection is held
only in memory. It never changes `profiles/installations.json`, the Tauri current
installation, game files or enabled mod state. A game without a provider mapping is
shown as unavailable rather than silently browsing DELTARUNE instead. The Mod Shop
also has an explicit game picker populated from the packaged game catalogue.

The mod library remains **profile-wide** because that is the existing runtime's
storage model. Filter by enabled, disabled or unknown state, narrow to runtime or
legacy packet format, and sort by name or enabled-first. Search and filters survive
screen changes. Unknown or malformed enabled state is never presented as disabled.
Attaching or disconnecting a profile clears profile-scoped filters and old shop
results. Late replies from the previous profile/search cannot replace current data.

**Settings → Disconnect profile** asks for confirmation, then clears the preview's
source reference and selection. It does not delete anything. Source profiles are
never remembered across application restarts. Invalid profile records produce an
error while leaving the last successfully attached source intact.

Keyboard shortcuts use **Cmd** on macOS and **Ctrl** on Windows/Linux:

| Shortcut | Action |
| --- | --- |
| Ctrl/Cmd + 1–6 | Home, library, installations, shop, themes, settings |
| Alt + Left / Right | Back / forward through up to 32 screen visits |
| Ctrl/Cmd + F | Focus library or shop search |
| Ctrl/Cmd + R | Refresh the attached profile |
| Ctrl/Cmd + Shift + O | Attach a profile through the native folder picker |
| F1 / Escape | Show shortcuts / close the current dialog |
| Tab / Shift + Tab | Move focus, contained within an open modal |

Rebuild the Rust host together with the frontend. The frontend now requires
`workspaceVersion: 2` and the explicit detach/selection capabilities during the
handshake. An old host is rejected with a rebuild message rather than failing later
when a new control is used. The presentation services require the matching native
host with `ui.preferences.get/set`, `theme.preview` and `shop.detail` capabilities.

## Interface, theme images and public mod details

Settings now offers **English** and **Italiano**. UI messages use a shared catalogue
with named placeholders, one-pass interpolation and English fallback. User mod and
game names, descriptions, filenames and diagnostic messages are not translated or
rewritten. Native/provider errors without a catalogue entry retain their original
text. These two languages do not establish parity with all Tauri languages.

The new language and image preferences are saved in `interface.json` under the
owned preview state directory, separate from the original appearance preferences.
Only acknowledged writes update the UI. No existing Tauri preference is migrated.

**Theme images** defaults off. When enabled, Home and Themes request only the
selected built-in theme background. The native service accepts a single bundled
PNG, JPEG or WebP filename of at most 12 MiB. It rejects linked files, URLs,
traversal, SVG and arbitrary media paths. Video, audio and custom CSS remain in
Tauri. A failed image is displayed as an error with explicit retry, not retried
in an idle loop.

**View mod** now opens public details in the preview: author, game, plain-text
description and bounded file metadata. The Rust network runtime still owns URL
containment, redirects, timeouts and response limits. The response never exposes
provider download URLs or tokens to the UI. Opening a detail does not download or
install a mod. Browser opening is a separate explicit action. Closing a detail,
changing game/profile or disposing the application invalidates late replies.

## Reviewed preview links

Use **Settings → Open a link**, or supply one at launch:

```sh
npm run dev -- --open 'https://gamebanana.com/mods/42'
npm run dev -- --open 'deltamod-gpuix-preview://screen?route=themes'
npm run dev -- --open 'deltamod-gpuix-preview://browse?game=toby.undertale&q=snow'
npm run dev -- --open 'deltamod-gpuix-preview://mod?id=42'
```

Each link is parsed into a bounded read-only intent and shown for confirmation.
Reviewing it performs no network request or navigation. Confirmation may navigate
to a screen, browse a catalogue-mapped game or fetch public mod details. Unsupported
schemes, credentials, unknown/duplicate parameters, paths and malformed IDs are
rejected. Links cannot launch games, write files, execute commands or install mods.

This is **CLI and paste handling only**. It does not register an OS protocol,
forward second-instance links, or replace the production `deltamod-community`
handler. The preview scheme is separate. `--open` cannot be combined with the
benchmark marker option because that would change the measured startup workload.

## Validation

```sh
npm test
npm run typecheck
npm run test:native
npm run smoke -- /absolute/path/to/new-evidence-directory
```

The first command tests real transport/model code using a fake child process. It
does not replace Cargo compilation or native GPU testing. The smoke command drives
the actual GPUIX window with a disposable two-installation profile. It captures all
six routes, selects a preview installation, exercises enabled/unknown library
filters and disconnects the profile. It hashes the source fixture before and after
these actions and records the result with the screenshots. Native host tests also
cover rejected/duplicate records, failed-attachment rollback, game ID containment
and unknown enabled state. These commands still require actual native execution.
They do not assert provider uptime, media playback or mutating-feature parity.

`tests/presentation.test.mjs` covers the real presentation model, link parser,
message catalogues and added transport allowlist with fake native responses. Its
passing results are not a claim that the Rust service or GPUI renderer ran.

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
3. Complete accounts, additional localization, accessibility, controller input, real
   media, OS-registered deep links and trusted packaging/update adapters.
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
