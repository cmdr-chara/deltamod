# Original frontend refinement

This change starts from `DeltaMaster` at `30fcbc3b47c022ec9f7cc6a7ca989a17328290c1`, not PR #58.
The compact icon rail, Pixel typography, theme backgrounds, soul/gear artwork, glass panels,
radial language picker, navigation registry, and native command contracts are retained.
There is no replacement shell, UI framework, Rust migration or dependency upgrade.

## Delivered behavior

- Patch menu and installed library: local search across name, authors, version and package ID.
  Sorting and game filtering reuse rows, preserving toggle and variant state and avoiding another
  native catalogue request. Author grouping remains available. A failed load has a retry action.
- Artwork metadata is requested near the viewport with four concurrent requests at most; navigation
  disconnects the observer, drops queued requests, and ignores late results. Game names are cached
  per library visit. Existing thumbnail placeholders are preserved on errors.
- Patch toggles disable during writes and roll back after failure. Launch waits for initial
  state loading and pending toggle writes; repeated launch clicks are guarded.
- Installation names preserve literal text, skip unchanged writes, accept Enter, cancel on Escape,
  and restore the previous name on write failure. Settings expose saving/saved/error feedback,
  accessible labels and failed-write rollback without changing their category layout.
- Alerts preserve the original alignment, sounds and art. Their queue drains after both acceptance
  and rejection, supports falsy response values, traps keyboard focus, inerts background controls,
  and restores focus. Escape only activates an explicitly rejecting choice; it never guesses a
  destructive confirmation. Optional sound failures cannot leave a dialog pending.
- Shop suggestions debounce input, cancel stale requests, encode query parameters, use the selected
  game's GameBanana ID, and support arrow/Enter selection. Provider search flows remain intact.
- Patch logs render text safely, batch DOM appends, keep at most 300 lines, and do not pull the
  reader away from older logs. Progress supports determinate, indeterminate and completed states.
- Theme names wrap in their existing full-width illustrated rows; medium-width action placement,
  shared focus states, log scrollbars and reduced-motion handling receive targeted fixes.
- All new control labels are supplied in the eight existing languages. Icons use the repository's
  existing bundled font stylesheet rather than requiring a Google Fonts request.

## Verification

```sh
npm ci
npm run typecheck
npm run build:boot
cargo build --workspace --locked --manifest-path native/Cargo.toml
npm test
npx playwright install chromium
npx playwright test tests/e2e/frontend-refinement.spec.js tests/e2e/tauri-themes.spec.js tests/e2e/tauri-localization.spec.js
```

The new browser fixture runs real shell markup, styles and view scripts with mocked native IPC.
It covers searching/sorting without rescans, state retention, image deferral and cancellation,
settings and rename failures, alert queuing/focus, progress/log limits, localization and desktop
widths of 800 and 1440 pixels. It does not claim native-window or live provider integration coverage.
Screenshots from these tests are retained by the dedicated frontend workflow.

The offline source/dependency snapshot omits native binaries and audio assets: its full suite has
48 failure reports on both the untouched baseline and the refinement. Those are not claimed as a
passing full run. CI checks the complete checkout instead. No changes to `DeltaMaster` are required
before review; the branch can be discarded without changing application data.

## Run locally

```sh
git fetch origin
git switch frontend-refinement
git pull --ff-only origin frontend-refinement
npm ci
npm run tauri:dev
```

The normal Tauri prerequisites and resource preparation described in the project README still apply.
