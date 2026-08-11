# Deltamod Tools Runtime

This crate is intentionally independent of the repository. It does not invoke a shell: every launch is a `std::process::Command` with an executable path, typed argv, explicit cwd, and an allowlisted environment.

## Command contracts

- G3MTool apply: `patch apply <backup> <source> <target-relative-to-game-root>`, cwd is the game root.
- G3MTool merge: `patch merge <backup> <source>... -a <target>`, cwd is the game root.
- UndertaleModCli: `load <input> --verbose --output <output> --scripts <script>...`, cwd is the CLI executable directory.
- WinUI editor: `--open <data-file>`, cwd is the editor executable directory.
- Folder reveal: Windows `explorer.exe <folder>`, macOS `open <folder>`, Linux `xdg-open <folder>`.

Tool binaries must be regular, non-symlink, single-link files. `verify_tool` hashes the canonical file with SHA-256 and rejects a pinned-hash mismatch before launch. `ProcessRegistry` owns spawned children and `run_bounded` enforces a timeout and bounded result. Windows processes are assigned to a `fence-windows` kill-on-close job; Unix processes are placed in their own process group and terminated through safe `rustix` APIs.

`legacy::map` preserves the existing adapter shape of `Result<T, String>` without making the domain API stringly typed.
