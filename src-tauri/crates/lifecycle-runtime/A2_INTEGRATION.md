# A2 lifecycle maintenance integration

This crate-local slice deliberately has no central IPC, Tauri `main.rs`, web UI,
provider implementation, or workspace-manifest wiring.

## Runtime entry points

- `ReleaseARuntime::update` accepts only a validated `Update` plan and uses the
  A1 journal, mutation lease, staging, backup, output verification, and manifest
  publication path. A recoverable filesystem failure rolls back immediately and
  terminalizes the operation as failed. Store uncertainty, lease loss, process
  death, or an externally changed destination remains `RecoveryRequired` so the
  runtime never overwrites evidence it no longer owns.
- `verify_installation` maps no-follow observations, expected hashes, durable
  ownership, manifest races, and interrupted operations into the frozen
  `VerificationResult` and `GameHealthReport` contracts.
- `verify_installation_with_sources` additionally reports whether each installed
  archive identity resolves to an exact cached archive or exact provider
  artifact/version tuple.
- `plan_repair` permits automatic repair only for missing lifecycle-owned files.
  It chooses an exact cached archive first, then the exact installed provider
  artifact identity. Hash changes, ownership conflicts, interrupted operations,
  unpinned provider versions, and unavailable exact sources fail explicitly.
- `repair` rechecks the frozen manifest generation and destination observations,
  then stages and publishes through the same journal boundary as update.
- `restore_last_working_state` durably protects the selected non-restore recovery
  generation before reading its backup journal. Protection survives interruption
  and is released only when the restore operation terminalizes or abandoned
  preflight is recovered. Restore operations do not become later restore sources,
  preventing version toggling from distinct restore requests.
- `operation_status` projects durable operation/journal state into
  `OperationProgress` and exposes the journal's deterministic recovery
  disposition.

Manifest-only observations needed for publication are persisted beside the
recovery generation. Forward execution and startup finalization both revalidate
them, so an unchanged/co-owned path cannot be replaced between preflight and
manifest publication without blocking the commit.

## Required integration wiring

1. Adapt the real filesystem workspace to `LifecycleWorkspace` and
   `LifecycleFilesystemBoundary`; it remains the only component allowed to
   publish, remove, or restore destination files.
2. Populate `RepairSourceCatalog` from the cache index and provider resolver.
   Catalog lookup is read-only: download/authentication work must resolve an
   exact artifact before `plan_repair` is called, and the returned `source_id`
   must address bytes available to `stage_file`.
3. Add narrow IPC handlers for verification, planning, repair/update execution,
   restore, and operation status. Preserve operation IDs and idempotency keys
   supplied by the caller; do not synthesize replacements on retry.
4. Feed `protected_by_operations` into recovery-retention accounting as active
   journal references. Eviction must continue to protect active, unfinished,
   pinned, latest-policy, and sole viable generations.
5. Apply the existing free-space policy before execution. The frozen installed
   file contract does not persist per-file sizes, so A2 repair plans report zero
   staging bytes; the cache/provider index must provide the exact archive's size
   estimate before central wiring enables mutation.
6. Stream `operation_status` snapshots to the existing progress contract; no new
   wire schema is required.

## Rollback and residual boundary

The prior manifest is never published over until every new output and unchanged
claim has been revalidated. Backups are retained as the completed recovery
generation. A normal filesystem failure is synchronously rolled back. If the
store cannot prove current lease/journal authority, or an external actor changes
a destination during rollback, the runtime intentionally stops with
`RecoveryRequired`; startup recovery then follows the journal disposition without
silently replacing the external file.
