# Legacy launchers

These scripts are retained for the Electron rollback path and older local
workflows. They are intentionally kept separate from the maintained Node and
Tauri commands documented in the root `README.md`.

- `run-no-dev.cmd` — launch the Electron app without developer tools.
- `run-controller-dev.cmd` — launch Electron controller developer mode.
- `run-tests.cmd` — run the JavaScript test suite.
- `run-linux.sh` — install/run the older Linux Electron workflow.
- `install-dependencies-release.sh` — install dependencies for the old release workflow.
- `install-build-dependencies.ps1` — install the historical Windows toolchain.
- `erase-data.bat` — invoke the explicit data-erasure helper (`npm run erase-data`).

Each launcher resolves the repository root relative to its own location, so
moving the scripts does not change their behavior.
