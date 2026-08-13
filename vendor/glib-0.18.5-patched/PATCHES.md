# Local GLib patch

This copy of `glib` 0.18.5 contains the upstream fix for the unsound
`VariantStrIter` iterator implementation reported as GHSA-wrw7-89jp-8q8g.

The patch is the two-line change from gtk-rs-core commit `b5a4071` (the same
fix shipped in `glib` 0.20.0): the pointer passed to `g_variant_get_child`
is mutable and the API receives `&mut p`.

Tauri 2's Linux shell currently depends on GTK3, whose `gtk` 0.18 series
requires GLib 0.18, so upgrading this dependency to the published 0.20
release is not yet compatible. Keep this patch until the GTK/GLib dependency
chain can move together; remove the vendor override when that happens.
