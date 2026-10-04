//! macOS application bundle upkeep for Deltamod's managed game copy.
//!
//! Replacing files under `Contents/Resources` invalidates the bundle's code
//! signature seal, and a copy of a downloaded app inherits its quarantine flag.
//! Both are repaired with Apple's own system tools; on other platforms these
//! functions do nothing.

use std::path::{Path, PathBuf};

/// The `.app` bundle that owns `content_root` (e.g. `DELTARUNE.app/Contents/Resources`).
#[must_use]
pub fn bundle_of(game_root: &Path, content_root: &str) -> Option<PathBuf> {
    let bundle = content_root.split('/').next()?;
    (bundle.len() > 4 && bundle.ends_with(".app") && !bundle.starts_with('.'))
        .then(|| game_root.join(bundle))
}

/// Removes the download quarantine flag from a copied bundle so Gatekeeper does
/// not reject it after Deltamod re-signs it. Failure is logged, not fatal.
pub fn clear_quarantine(bundle: &Path) {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(bundle)
            .status();
        if !status.is_ok_and(|status| status.success()) {
            eprintln!("[mac] could not clear quarantine on {}", bundle.display());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = bundle;
}

/// Ad-hoc re-signs a bundle after its resources changed, so macOS accepts it.
pub fn reseal(bundle: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--deep", "--sign", "-"])
            .arg(bundle)
            .output()
            .map_err(|error| format!("codesign could not start: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "codesign failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(512)
                    .collect::<String>()
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = bundle;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_is_the_first_app_component_of_the_content_root() {
        let root = Path::new("/games");
        assert_eq!(
            bundle_of(root, "DELTARUNE.app/Contents/Resources"),
            Some(root.join("DELTARUNE.app"))
        );
        assert_eq!(bundle_of(root, "assets"), None);
        assert_eq!(bundle_of(root, ".app/Contents"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reseal_produces_a_verifiable_ad_hoc_signature() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Test.app");
        let macos = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>run</string>
<key>CFBundleIdentifier</key><string>test.deltamod.reseal</string>
</dict></plist>"#,
        )
        .unwrap();
        std::fs::copy("/usr/bin/true", macos.join("run")).unwrap();
        std::fs::write(bundle.join("Contents/Resources/game.ios"), b"v1").unwrap();
        reseal(&bundle).unwrap();
        std::fs::write(bundle.join("Contents/Resources/game.ios"), b"v2").unwrap();
        let verify = |bundle: &Path| {
            std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep"])
                .arg(bundle)
                .status()
                .unwrap()
                .success()
        };
        assert!(!verify(&bundle), "changed resources break the seal");
        reseal(&bundle).unwrap();
        assert!(verify(&bundle));
    }
}
