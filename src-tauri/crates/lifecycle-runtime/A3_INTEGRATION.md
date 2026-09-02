# A3 profile integration boundary

`profiles.rs` is orchestration-plan-only. It imports/exports exact local profile
documents, diffs a target lockfile against `InstallationManifest`, validates all
resolved install/update plans together, and emits deterministic existing-operation
steps plus idempotency inputs. It performs no filesystem or store mutation.

Every emitted child step has a plan-derived operation ID and idempotency key;
preflight accepts exactly one resolved install/update plan per expected instance
and requires both values to match. The source installation ID, generation, and
exact manifest fingerprint are frozen into the switch plan and rechecked.

Preflight also builds a deterministic claim-transition dependency order. Releases
and compatible owner updates precede dependent acquisitions. A cyclic co-owner
replacement that cannot be represented safely by the existing single-item
lifecycle operations fails closed instead of emitting an executable order.

`ReleaseARuntime::switch_profile` is the parent executor. It accepts one outer
`ProfileSwitch` request whose file-plan fingerprint is the exact
`ProfileSwitchPlan` fingerprint. Child operation IDs remain deterministic plan
bindings; they are never acquired as independent operations. The executor:

1. acquires one installation lease;
2. runs the complete profile preflight plus authoritative no-follow filesystem
   observations before any workspace mutation;
3. simulates child uninstall/update/install transitions in the plan's
   dependency-safe order and reduces them to the net manifest plus one ordered
   mutation list;
4. stages, backs up, applies, and verifies that list through one Release-A
   journal; and
5. publishes the manifest and `ActiveProfilePointer` in the same durable store
   frame only after every destination verifies.

Any recoverable forward filesystem failure enters rollback immediately. Profile
rollback traverses the one journal in reverse order and leaves both the previous
manifest and previous active pointer published. An interrupted batch is rebound
by `recover_startup`: incomplete effects roll back, while a batch whose complete
output already verified finalizes the manifest and pointer atomically. Replaying
the same outer request returns the durable operation without applying child
effects again. A switch with only ownership/order/pointer changes uses the
manifest-only atomic store boundary after revalidating every retained claim.

Host wiring must build the outer `OperationRequest`, resolve every install/update
child exactly once, and call `switch_profile`; it must not invoke `install`,
`update`, or `uninstall` for the child plan operations.
