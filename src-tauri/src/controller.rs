use deltamod_tools_runtime::{
    controller_mode_launch, verify_tool, OwnedProcess, ProcessRegistry, ToolKind,
};
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
};
use tauri::AppHandle;

const CONTROLLER_FLAG: &str = "-controller";
const CONTROLLER_SHA256: &str = "04ACDBB53C96CD99B01FE53A0297AC06308DDAD14B5253A3AF4F9A319985AA45";

pub struct ControllerMode {
    enabled: bool,
    executable: std::path::PathBuf,
    registry: ProcessRegistry,
    process: Mutex<Option<OwnedProcess>>,
}

impl ControllerMode {
    pub fn new(resources: &Path) -> Self {
        let enabled =
            cfg!(target_os = "windows") && std::env::args_os().any(|arg| arg == CONTROLLER_FLAG);
        Self {
            enabled,
            executable: resources.join("tools").join("cmodeutil.exe"),
            registry: ProcessRegistry::default(),
            process: Mutex::new(None),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn start(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut process = self
            .process
            .lock()
            .map_err(|_| "controller mode unavailable")?;
        if process.is_some() {
            return Ok(());
        }
        let tool = verify_tool(
            &self.executable,
            ToolKind::ControllerMode,
            Some(CONTROLLER_SHA256),
        )
        .map_err(|_| "controller mode unavailable")?;
        *process = Some(
            self.registry
                .spawn_silent(&controller_mode_launch(&tool))
                .map_err(|_| "controller mode unavailable")?,
        );
        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(mut process) = self.process.lock() {
            if let Some(process) = process.take() {
                let _ = process.terminate();
            }
        }
    }
}

impl Drop for ControllerMode {
    fn drop(&mut self) {
        self.registry.terminate_all();
    }
}

fn is_protocol_url(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.starts_with("deltamod://") || value.starts_with("deltamod-community://")
}

fn relaunch_args(args: impl IntoIterator<Item = OsString>, controller: bool) -> Vec<OsString> {
    let mut filtered: Vec<_> = args
        .into_iter()
        .filter(|arg| arg != CONTROLLER_FLAG)
        .filter(|arg| arg.to_str().is_none_or(|arg| !is_protocol_url(arg)))
        .collect();
    if controller {
        filtered.push(CONTROLLER_FLAG.into());
    }
    filtered
}

pub fn relaunch(app: &AppHandle, controller: bool) -> Result<(), String> {
    if !cfg!(target_os = "windows") {
        return Err("TAURI_COMMAND_UNAVAILABLE:controller-mode".into());
    }
    let executable = std::env::current_exe().map_err(|_| "controller mode unavailable")?;
    let args = relaunch_args(std::env::args_os().skip(1), controller);
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "controller mode unavailable")?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_arguments_drop_mode_and_protocol_urls() {
        let args = [
            "--developer",
            "-controller",
            "deltamod://install/one",
            "DELTAMOD-COMMUNITY://install/two",
            "--safe",
        ]
        .into_iter()
        .map(OsString::from);
        assert_eq!(
            relaunch_args(args, true),
            vec!["--developer", "--safe", "-controller"]
        );
    }

    #[test]
    fn controller_hash_is_pinned() {
        assert_eq!(CONTROLLER_SHA256.len(), 64);
        assert!(CONTROLLER_SHA256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
    }
}
