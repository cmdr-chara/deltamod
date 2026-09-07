# Desktop refinement platform validation

This document records the platform scope for the `frontend-refinement` work.

- Linux and Windows are first-class desktop targets for the refined interface.
- The original Deltamod visual identity, theme system, compact navigation rail, and language wheel remain the product baseline.
- Linux-specific rendering, navigation, Mod Shop, font, and theme behavior must remain covered by automated browser/native checks where the CI environment supports them.
- Windows behavior must remain covered by the repository's Windows build and browser checks.
- Tauri/Rust is the preferred desktop runtime path; compatibility code must not be described as removed until the repository and release workflows prove that it is no longer required.
- Browser tests that mock native IPC are evidence for frontend behavior, not a substitute for native Tauri verification.

The pull request should only be considered ready after its final head commit has completed the applicable repository CI checks.
