# Deltamod Tools Runtime

This crate is intentionally independent of the repository. It does not invoke a shell: every launch is a `std::process::Command` with an executable path, typed argv, explicit cwd, and an allowlisted environment.

## Command contracts

- G3MTool apply: `patch apply <backup> <source> <target-relative-to-game-root>`, cwd is the game root.
- G3MTool merge: `patch merge <backup> <source>... -a <target>`, cwd is the game root.
- UndertaleModCli: `load <input> --verbose --output <output> --scripts <script>...`, cwd is the CLI executable directory.
- WinUI editor: `--open <data-file>`, cwd is the editor executable directory.
- Folder reveal: Windows `explorer.exe <folder>`, macOS `open <folder>`, Linux `xdg-open <folder>`.

Tool binaries must be regular, non-symlink, single-link files. `verify_tool` records the SHA-256 digest in `ToolPath`; command helpers add an internal identity-plus-digest launch pin to `CommandSpec` without changing its public fields. Manually constructed specifications can opt in through `CommandSpec::pin_to`. The pin is removed from the child environment before spawn. `ProcessRegistry` rejects unpinned commands and rechecks identity plus digest at launch. Linux launches the pinned descriptor through `/proc/self/fd`; other Unix targets retain and recheck the open descriptor. Windows uses `fence-windows` identity snapshots immediately before and after `CreateProcess`, because its current node handle cannot remain open across a cold launch.

`run_bounded` keeps its existing signature. `run_bounded_with_cancel` adds an explicit cloneable `CancellationToken`. Timeout, cancellation, and aggregate stdout-plus-stderr overflow terminate the full process tree, wait for the root child, drain and join both output readers, and return distinct `RuntimeError::{Timeout, Cancelled, OutputOverflow}` variants. Residual descendants are also terminated after a normal root exit before success is returned. `ProcessOutput::timed_out` and `ProcessOutput::truncated` remain for source compatibility but are false on successful results; bounded-run policy failures are errors.

Windows processes are assigned to a `fence-windows` kill-on-close job; Unix processes are placed in their own process group and terminated through safe `rustix` APIs. `OwnedProcess::terminate`, `OwnedProcess::drop`, and `ProcessRegistry::terminate_all` perform the same blocking terminate-and-reap path. Bare system launcher specifications such as `reveal_folder` remain unpinned and are therefore not accepted by the hardened registry without an explicit verified executable pin.

`legacy::map` preserves the existing adapter shape of `Result<T, String>` without making the domain API stringly typed.
