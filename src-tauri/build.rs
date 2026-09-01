fn main() {
    // Tauri copies bundled resources next to the development executable when this
    // build script runs. Track directory contents explicitly so adding or removing
    // a theme/game definition invalidates that staging step instead of leaving a
    // stale target/debug resource tree behind.
    println!("cargo:rerun-if-changed=../web/themes");
    println!("cargo:rerun-if-changed=../games");
    tauri_build::build()
}
