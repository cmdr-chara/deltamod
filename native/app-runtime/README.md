# Shared application runtime

`deltamod-app-runtime` compiles the canonical windowless backend, state,
transactional lifecycle/import/patch/launch channels and Nexus PKCE implementation.
GPUIX depends on this package, not on the Tauri shell package. The old extern-crate
alias is retained at the host's Cargo boundary to avoid unrelated source churn.

The source files remain at their existing paths so both shells compile one
implementation and source-relative resources keep their meaning. There is no
copied transaction engine, fake Tauri type, shell build script, or renderer in
this package. Source relocation is not needed to obtain an independent build.

Only the real Tauri shell build sets `deltamod_tauri_shell`. That local cfg includes
its window lifecycle, updater and plugin-backed wrappers. It is not a Cargo feature
that can be enabled accidentally through dependency-feature unification. The
independent package defines no Tauri or WebView dependency, including optional ones.

`node desktop-gpuix/scripts/check-backend.mjs` checks the resolved normal/build
closure, following actual package IDs rather than trusting dependency aliases.
A successful locked Cargo compile and installed smoke remain required. Merely
inspecting the manifest does not prove this extraction compiles on any target.
