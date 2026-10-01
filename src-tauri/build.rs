fn main() {
    // This cfg is private to this compilation unit, not a feature that can be
    // unified into the independent GPUIX runtime by a workspace consumer.
    println!("cargo:rustc-check-cfg=cfg(deltamod_tauri_shell)");
    println!("cargo:rustc-cfg=deltamod_tauri_shell");
    // Tauri copies bundled resources next to the development executable when this
    // build script runs. Track directory contents explicitly so adding or removing
    // a theme/game definition invalidates that staging step instead of leaving a
    // stale target/debug resource tree behind.
    println!("cargo:rerun-if-changed=../web/themes");
    println!("cargo:rerun-if-changed=../games");
    tauri_build::build()
}
