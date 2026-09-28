# Continuous integration

## Check ownership

| Check | Work it owns |
| --- | --- |
| Community CI / quality | Tauri-only and security policies, manifests, typecheck, static IPC contracts, both workspace formatting checks, workflow lint and result-gate tests |
| Community CI / verify | Native workers and full Tauri workspace Clippy/tests, boot build, target resource staging and JavaScript tests on Windows x64, Linux x64, Intel macOS and Apple Silicon |
| Original frontend refinement | Boot build and all browser E2E specs on Linux and Windows. It no longer rebuilds workers or repeats the unit suite |
| macOS verify jobs | All browser E2E specs and platform tool smoke checks. Apple Silicon's existing CSX restriction remains explicit |
| Community Provenance / Secure Updater Policy | Independent provenance and signed-updater policy checks |
| Native CI result | Fails unless both `quality` and the entire `verify` matrix succeed. Missing, skipped, cancelled and unknown results do not pass |

The aggregate result covers Community CI, not the separate frontend, provenance or
updater workflows. Existing check names are preserved where possible. Repository
branch-protection configuration is not changed by these source files.

## Cost and reproducibility

- New pushes cancel superseded PR runs, not main-branch release preparation.
- Successful same-repository PR builds can save Rust caches under GitHub's isolated
  PR merge refs. Fork builds cannot save them. Rust version, platform and dependency
  inputs remain part of the cache key. Caches are never acceptance evidence.
- Browser jobs own browser tests only. Each native platform still runs its complete
  native and JavaScript checks. Tools acquired and staged in a job are verified for
  smoke tests without a second acquisition.
- Every job has a deadline, with shorter bounds on dependency/browser downloads.
  Failing tests are not retried until green. The existing retry wrapper is used only
  for external dependency acquisition.
- CI actions are pinned to their reviewed commit IDs. Dependabot tracks GitHub
  Actions separately from npm and Cargo dependencies. Workflow syntax/expression
  lint uses actionlint 1.7.12 with an archive SHA-256 check before extraction.
  It covers the validation workflows plus the Dependabot merge policy. ShellCheck
  and Pyflakes integration are disabled rather than installed implicitly.

These remove redundant work. They do not claim a measured end-to-end speedup on
comparable hosted runners or change desktop benchmarks.

## Dependabot auto-merge

`.github/workflows/dependabot-automerge.yml` listens only to completed validation
workflow runs. It does not check out or execute pull-request code with its write
token. Before merging, it resolves the exact workflow head SHA back to one open,
non-draft PR authored by `dependabot[bot]`, targeting `DeltaMaster` from this
repository.

Community CI, Community Provenance and Secure Updater Policy must all have a current
successful pull-request run for that exact head. Original frontend refinement is
also required when the changed paths match that workflow's path filter. Missing,
pending, failed, cancelled or ambiguous evidence leaves the PR open. The head SHA
and bot/base identity are checked again immediately before the merge request.

GitHub still owns final mergeability and repository-rule enforcement. Conflicted or
otherwise unmergeable Dependabot PRs remain open. Human-authored PRs are never
eligible for this workflow.

## Diagnostics and local reproduction

Each native job uploads `ci-evidence-<target>-<run>-<attempt>` for seven days,
including the checked revision, tool versions, Clippy/test logs, Vitest JUnit and
available Playwright evidence. Failure exit codes survive log capture through
`pipefail`. No complete environment, credentials or user installation is captured.
Browser jobs retain JUnit and failure traces in their own evidence artifact.

Start with the failing step and exact checked revision, not the current branch tip.
For the result gate itself:

```console
node --test scripts/ci/gate.test.cjs
```

Two platform failures found during this CI audit have regression coverage:

- Windows no-follow reads now distinguish an absent optional `__variant` file from
  identity replacement, permission denial or an unsafe path. Actual identity races
  remain errors. The old directory helper conflated absent children with replacement.
- The loopback updater fixture explicitly sets accepted TCP streams to blocking mode
  with read/write deadlines. BSD/macOS can inherit a listener's nonblocking state,
  unlike Linux. A forced-nonblocking regression checks the fixture without changing
  production cancellation, signature or timeout policy.

## Optional benchmark and releases

The expensive unsigned Windows installer benchmark is opt-in through Community CI's
manual `benchmark` input, after native verification succeeds. It no longer starts
because an unrelated test failed. Once this workflow is on the default branch:

```console
gh workflow run ci.yml --ref <branch-or-sha> -f benchmark=true
```

This diagnostic job cannot publish a release. Its existing raw performance evidence
remains separate from the same-host release benchmark and signed release gates.
Stable-release preparation runs only after a successful main-branch native result,
checks out the exact validated SHA, refuses a stale candidate and serializes tag
preparation. When every signing credential is present it may create the immutable
stable tag and dispatch the signed release. If any signing credential is absent, it
does not create a stable tag. Instead it dispatches `Community Tauri Release` in
`validation` mode for that exact SHA.

Release validation builds and installs the Windows, Linux, Intel macOS and Apple
Silicon packages, runs the packaged smoke/protocol/sidecar checks, verifies that the
candidate contains no updater signatures or `latest.json`, and retains a SHA-256
manifest for seven days. Validation artifacts are intentionally unsigned and are not
published. A green validation run is packaging evidence, not authorization to call a
stable release signed or publishable.

Unsigned preview tags may start the release workflow either from their tag push or
through an explicit workflow dispatch against the immutable preview tag. The dispatch
path exists because tags created with a workflow's `GITHUB_TOKEN` do not recursively
trigger another workflow run.
