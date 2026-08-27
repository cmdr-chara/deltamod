use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Once,
};

const MODE_VAR: &str = "DELTAMOD_LINUX_WEBKIT_RENDERER";
const FORCE_SHM_VAR: &str = "WEBKIT_DMABUF_RENDERER_FORCE_SHM";
const DISABLE_DMABUF_VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const DISABLE_COMPOSITING_VAR: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
const RENDER_DEVICE_VAR: &str = "WEBKIT_WEB_RENDER_DEVICE_FILE";
const EGL_VENDOR_VAR: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";
const EGL_VENDOR_DIRS_VAR: &str = "__EGL_VENDOR_LIBRARY_DIRS";

static CONFIGURE: Once = Once::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestedMode {
    Auto,
    Native,
    SharedMemory,
    DisableDmabuf,
}

impl RequestedMode {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Native => "native",
            Self::SharedMemory => "shm",
            Self::DisableDmabuf => "disable-dmabuf",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct GraphicsProbe {
    has_intel_or_amd_drm: bool,
    has_nvidia_drm: bool,
    has_native_nvidia_userspace: bool,
    mesa_egl_vendor: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExistingEnvironment {
    webkit_renderer_overridden: bool,
    egl_vendor_overridden: bool,
    gpu_route_overridden: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnvironmentPlan {
    mode: RequestedMode,
    risk_detected: bool,
    pin_mesa_egl: Option<PathBuf>,
    force_shm: bool,
    disable_dmabuf: bool,
    reason: &'static str,
}

fn parse_mode(raw: Option<&str>) -> (RequestedMode, bool) {
    let normalized = raw.unwrap_or("auto").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "auto" => (RequestedMode::Auto, true),
        "native" | "default" => (RequestedMode::Native, true),
        "shm" | "shared-memory" | "shared_memory" => (RequestedMode::SharedMemory, true),
        "disable" | "disable-dmabuf" | "legacy" => (RequestedMode::DisableDmabuf, true),
        _ => (RequestedMode::Auto, false),
    }
}

fn plan_environment(
    probe: &GraphicsProbe,
    existing: ExistingEnvironment,
    mode: RequestedMode,
) -> EnvironmentPlan {
    let risk_detected =
        probe.has_intel_or_amd_drm && !probe.has_nvidia_drm && probe.has_native_nvidia_userspace;

    let mut plan = EnvironmentPlan {
        mode,
        risk_detected,
        pin_mesa_egl: None,
        force_shm: false,
        disable_dmabuf: false,
        reason: "no mixed-vendor WebKitGTK risk detected",
    };

    match mode {
        RequestedMode::Native => {
            plan.reason = "native renderer explicitly requested";
        }
        RequestedMode::SharedMemory => {
            if !existing.webkit_renderer_overridden {
                plan.force_shm = true;
                plan.reason = "shared-memory renderer explicitly requested";
            } else {
                plan.reason = "existing WebKit renderer override preserved";
            }
        }
        RequestedMode::DisableDmabuf => {
            if !existing.webkit_renderer_overridden {
                plan.disable_dmabuf = true;
                plan.reason = "legacy DMA-BUF disable explicitly requested";
            } else {
                plan.reason = "existing WebKit renderer override preserved";
            }
        }
        RequestedMode::Auto if risk_detected && existing.webkit_renderer_overridden => {
            plan.reason = "existing WebKit renderer override preserved";
        }
        RequestedMode::Auto if risk_detected && existing.gpu_route_overridden => {
            plan.reason = "explicit GPU routing override preserved";
        }
        RequestedMode::Auto if risk_detected && existing.egl_vendor_overridden => {
            plan.reason = "existing EGL vendor override preserved";
        }
        RequestedMode::Auto if risk_detected => {
            plan.pin_mesa_egl = probe.mesa_egl_vendor.clone();
            plan.reason = if plan.pin_mesa_egl.is_some() {
                "non-NVIDIA DRM GPU with native NVIDIA userspace detected; pinning Mesa EGL"
            } else {
                "mixed-vendor risk detected but no safe automatic EGL pin is available"
            };
        }
        RequestedMode::Auto => {}
    }

    plan
}

fn drm_probe(root: &Path) -> (bool, bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return (false, false);
    };

    let mut has_intel_or_amd = false;
    let mut has_nvidia = false;
    for entry in entries.flatten() {
        let Ok(vendor) = fs::read_to_string(entry.path().join("device/vendor")) else {
            continue;
        };
        match vendor.trim().to_ascii_lowercase().as_str() {
            "0x8086" | "0x1002" => has_intel_or_amd = true,
            "0x10de" => has_nvidia = true,
            _ => {}
        }
    }
    (has_intel_or_amd, has_nvidia)
}

fn mesa_egl_vendor() -> Option<PathBuf> {
    const DIRECTORIES: &[&str] = &["/etc/glvnd/egl_vendor.d", "/usr/share/glvnd/egl_vendor.d"];

    for directory in DIRECTORIES {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".json") {
                continue;
            }
            if name.contains("mesa") {
                return Some(path);
            }
            if fs::read_to_string(&path)
                .map(|contents| contents.to_ascii_lowercase().contains("libegl_mesa"))
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

fn directory_has_native_nvidia_library(directory: &str) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("libnvidia-eglcore.so")
            || name.starts_with("libnvidia-gpucomp.so")
            || name.starts_with("libEGL_nvidia.so")
    })
}

fn has_native_nvidia_userspace() -> bool {
    // Deltamod's supported Linux package is native x64. Deliberately do not
    // inspect /usr/lib32: a 32-bit-only NVIDIA package cannot be loaded by the
    // 64-bit WebKitWebProcess and must not activate this compatibility guard.
    const DIRECTORIES: &[&str] = &[
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
    ];
    DIRECTORIES
        .iter()
        .any(|directory| directory_has_native_nvidia_library(directory))
}

fn probe_graphics() -> GraphicsProbe {
    let (has_intel_or_amd_drm, has_nvidia_drm) = drm_probe(Path::new("/sys/class/drm"));
    GraphicsProbe {
        has_intel_or_amd_drm,
        has_nvidia_drm,
        has_native_nvidia_userspace: has_native_nvidia_userspace(),
        mesa_egl_vendor: mesa_egl_vendor(),
    }
}

fn existing_environment() -> ExistingEnvironment {
    ExistingEnvironment {
        webkit_renderer_overridden: env::var_os(FORCE_SHM_VAR).is_some()
            || env::var_os(DISABLE_DMABUF_VAR).is_some()
            || env::var_os(DISABLE_COMPOSITING_VAR).is_some()
            || env::var_os(RENDER_DEVICE_VAR).is_some(),
        egl_vendor_overridden: env::var_os(EGL_VENDOR_VAR).is_some()
            || env::var_os(EGL_VENDOR_DIRS_VAR).is_some(),
        gpu_route_overridden: env::var_os("__NV_PRIME_RENDER_OFFLOAD").is_some()
            || env::var_os("__NV_PRIME_RENDER_OFFLOAD_PROVIDER").is_some()
            || env::var_os("DRI_PRIME").is_some()
            || env::var_os("__GLX_VENDOR_LIBRARY_NAME").is_some()
            || env::var_os("GBM_BACKEND").is_some(),
    }
}

#[cfg(target_os = "linux")]
fn configure_linux() {
    let raw_mode = env::var(MODE_VAR).ok();
    let (mode, recognized) = parse_mode(raw_mode.as_deref());
    if !recognized {
        eprintln!(
            "[linux-webkit] ignoring unknown {MODE_VAR} value {:?}; using auto",
            raw_mode.as_deref().unwrap_or_default()
        );
    }

    let probe = probe_graphics();
    let existing = existing_environment();
    let plan = plan_environment(&probe, existing, mode);

    if let Some(path) = &plan.pin_mesa_egl {
        env::set_var(EGL_VENDOR_VAR, path.as_os_str());
    }
    if plan.force_shm {
        env::set_var(FORCE_SHM_VAR, "1");
    }
    if plan.disable_dmabuf {
        env::set_var(DISABLE_DMABUF_VAR, "1");
    }

    eprintln!(
        "[linux-webkit] mode={} mixed_vendor_risk={} intel_or_amd_drm={} nvidia_drm={} native_nvidia_userspace={} mesa_egl={} renderer={} compositing={} render_device={} egl_vendor={} reason={}",
        plan.mode.label(),
        plan.risk_detected,
        probe.has_intel_or_amd_drm,
        probe.has_nvidia_drm,
        probe.has_native_nvidia_userspace,
        probe
            .mesa_egl_vendor
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unavailable".to_owned()),
        effective_renderer_label(),
        effective_compositing_label(),
        effective_render_device_label(),
        env::var(EGL_VENDOR_VAR).unwrap_or_else(|_| "automatic".to_owned()),
        plan.reason,
    );
}

fn effective_renderer_label() -> &'static str {
    if env::var_os(FORCE_SHM_VAR).is_some() {
        "shared-memory"
    } else if env::var_os(DISABLE_DMABUF_VAR).is_some() {
        "dmabuf-disabled"
    } else {
        "native"
    }
}

fn effective_compositing_label() -> &'static str {
    if env::var_os(DISABLE_COMPOSITING_VAR).is_some() {
        "disabled"
    } else {
        "automatic"
    }
}

fn effective_render_device_label() -> String {
    env::var_os(RENDER_DEVICE_VAR)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "automatic".to_owned())
}

pub fn configure() {
    CONFIGURE.call_once(|| {
        #[cfg(target_os = "linux")]
        configure_linux();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(
        has_intel_or_amd_drm: bool,
        has_nvidia_drm: bool,
        has_native_nvidia_userspace: bool,
    ) -> GraphicsProbe {
        GraphicsProbe {
            has_intel_or_amd_drm,
            has_nvidia_drm,
            has_native_nvidia_userspace,
            mesa_egl_vendor: Some(PathBuf::from("/usr/share/glvnd/egl_vendor.d/50_mesa.json")),
        }
    }

    #[test]
    fn auto_mitigates_mixed_vendor_intel_or_amd_systems_without_disabling_acceleration() {
        let plan = plan_environment(
            &probe(true, false, true),
            ExistingEnvironment::default(),
            RequestedMode::Auto,
        );
        assert!(plan.risk_detected);
        assert!(!plan.force_shm);
        assert!(!plan.disable_dmabuf);
        assert_eq!(
            plan.pin_mesa_egl,
            Some(PathBuf::from("/usr/share/glvnd/egl_vendor.d/50_mesa.json"))
        );
    }

    #[test]
    fn auto_does_not_touch_real_nvidia_systems() {
        let plan = plan_environment(
            &probe(true, true, true),
            ExistingEnvironment::default(),
            RequestedMode::Auto,
        );
        assert!(!plan.risk_detected);
        assert!(!plan.force_shm);
        assert!(!plan.disable_dmabuf);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn auto_does_not_touch_clean_mesa_systems() {
        let plan = plan_environment(
            &probe(true, false, false),
            ExistingEnvironment::default(),
            RequestedMode::Auto,
        );
        assert!(!plan.risk_detected);
        assert!(!plan.force_shm);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn automatic_policy_preserves_existing_renderer_override() {
        let plan = plan_environment(
            &probe(true, false, true),
            ExistingEnvironment {
                webkit_renderer_overridden: true,
                egl_vendor_overridden: false,
                gpu_route_overridden: false,
            },
            RequestedMode::Auto,
        );
        assert!(plan.risk_detected);
        assert!(!plan.force_shm);
        assert!(!plan.disable_dmabuf);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn automatic_policy_preserves_existing_egl_override() {
        let plan = plan_environment(
            &probe(true, false, true),
            ExistingEnvironment {
                webkit_renderer_overridden: false,
                egl_vendor_overridden: true,
                gpu_route_overridden: false,
            },
            RequestedMode::Auto,
        );
        assert!(plan.risk_detected);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn auto_preserves_explicit_gpu_routing() {
        let plan = plan_environment(
            &probe(true, false, true),
            ExistingEnvironment {
                webkit_renderer_overridden: false,
                egl_vendor_overridden: false,
                gpu_route_overridden: true,
            },
            RequestedMode::Auto,
        );
        assert!(plan.risk_detected);
        assert!(!plan.force_shm);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn explicit_native_mode_is_an_opt_out() {
        let plan = plan_environment(
            &probe(true, false, true),
            ExistingEnvironment::default(),
            RequestedMode::Native,
        );
        assert!(!plan.force_shm);
        assert!(!plan.disable_dmabuf);
        assert!(plan.pin_mesa_egl.is_none());
    }

    #[test]
    fn explicit_shared_memory_and_legacy_modes_are_reversible() {
        let shared = plan_environment(
            &probe(false, false, false),
            ExistingEnvironment::default(),
            RequestedMode::SharedMemory,
        );
        assert!(shared.force_shm);
        assert!(!shared.disable_dmabuf);

        let legacy = plan_environment(
            &probe(false, false, false),
            ExistingEnvironment::default(),
            RequestedMode::DisableDmabuf,
        );
        assert!(!legacy.force_shm);
        assert!(legacy.disable_dmabuf);
    }

    #[test]
    fn mode_parser_falls_back_to_auto_for_unknown_values() {
        assert_eq!(parse_mode(Some("shm")), (RequestedMode::SharedMemory, true));
        assert_eq!(parse_mode(Some("native")), (RequestedMode::Native, true));
        assert_eq!(
            parse_mode(Some("something-else")),
            (RequestedMode::Auto, false)
        );
    }
}
