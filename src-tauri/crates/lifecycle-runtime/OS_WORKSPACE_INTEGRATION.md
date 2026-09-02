# Lifecycle filesystem boundary

`OsLifecycleWorkspace` is the only production adapter permitted to publish,
replace, or remove lifecycle-owned game files.

On Windows it uses `fence-windows` mutation roots. On Linux and macOS it pins
roots and destination parents by device/inode identity, traverses parents with
`openat(..., O_DIRECTORY | O_NOFOLLOW)`, creates temporary outputs with
`openat(..., O_EXCL | O_NOFOLLOW)`, and commits with descriptor-relative
`renameat`/`renameat_with`. Removal uses descriptor-relative `unlinkat`.

Every mutation is preceded by journal/lease validation, root and observation
revalidation, and expected-hash verification. The published file and its parent
directory are synced before the journal records the side effect. Linux path
identity remains case-sensitive; Windows remains case-insensitive.

Native Linux and macOS CI must run the adversarial workspace tests before a
package can satisfy Release E. A Windows-only build proves the shared contract
and Windows branch but is not evidence that Unix kernel behavior passed.

POSIX does not offer one portable operation that replaces or removes a name
only when it still identifies an expected inode. Descriptor-relative traversal
prevents path escape and link traversal; the application-level per-installation
lease supplies the required single-writer boundary. A malicious same-user
process racing the final filename is outside that cooperative lock and remains
a documented platform limitation rather than being silently treated as tested.
