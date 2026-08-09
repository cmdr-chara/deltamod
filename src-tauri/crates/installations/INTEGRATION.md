# Installation Domain Integration

This crate is pure domain code. The Tauri layer should deserialize the legacy command payload into `LegacyIpcRequest`, call `convert_request`, dispatch the typed `Request`, and serialize one of the response structs or stream `ProgressEvent` values to the renderer. Adapter errors should be mapped to the existing command error envelope; no adapter should manufacture success.

## Dispatcher mapping

| Legacy command | Typed request |
| --- | --- |
| list installations | `Request::List` |
| select installation | `Request::Select` |
| set name / edition | `Request::Name` / `Request::Edition` |
| import | `Request::Import` |
| reimport / repair / remove | corresponding operation request |

`GamePlatform::resolve` is the only host/Wine decision point. Use `patch_target` for every patch path; never concatenate unchecked renderer input. `Ownership::ManagedCopy` permits deleting the managed install path. `LinkedExternal` always produces `PreserveLinked` for deletion, including when the UI asks to delete files.

## Adapter dependencies

The dispatcher owns filesystem execution and should connect managed imports to `staged_copy`, patch restoration to the patch-restore service, and metadata persistence to storage. Reimport and repair should call `OperationRegistry::checkpoint` during work, then `commit` before the irreversible restore/delete boundary, and `finish` after the adapter result. Dialogs and process launches implement `DialogAdapter` and `ProcessAdapter`; this crate intentionally does neither.
