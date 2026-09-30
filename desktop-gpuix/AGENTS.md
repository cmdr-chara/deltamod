# GPUIX migration boundary

This directory is an opt-in, read-only application prototype. The production Tauri
application, release pipeline, updater trust and original profile files are not
owned by this stage.

- Use actual `@gpuix/react` APIs. Do not introduce React DOM, a hidden WebView,
  browser-only animation libraries or unsupported CSS properties as substitutes.
- The Rust host reads a user-selected source profile. It may write only inside its
  separately owned preview state directory. Do not instantiate a writable runtime
  on the source profile or treat a failed read as a successful empty library.
- New game/mod mutations require the existing transaction/recovery adapters and
  native integration evidence. A button or mocked test does not establish parity.
- Keep backend requests bounded, commands allowlisted and child processes owned.
- Native glass is platform-dependent. Current motion targets are native numeric
  tweens, not assumed springs, CSS keyframes or a full liquid-refraction shader.
- Tests in `tests/` are logic/transport checks. Native smoke, real GPUI screenshots,
  TypeScript package checks and Cargo compilation are distinct evidence.
- Freeze the new frontend and native dependency locks before reproducible benchmark
  or release use. Do not alter either existing Tauri lockfile to make this build.
- No automatic CI polling. No Tauri retirement without feature and native parity.
