#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};
use url::Url;

pub const MAX_URL_BYTES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterError {
    InvalidUrl,
    HostNotAllowed,
    InvalidFolder,
    FolderNotAllowed,
    Io,
    InvalidShortcut,
    InvalidDialog,
    InvalidSelection,
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidUrl => "invalid URL",
            Self::HostNotAllowed => "URL host is not allowed",
            Self::InvalidFolder => "folder does not exist",
            Self::FolderNotAllowed => "folder is not an approved backend folder",
            Self::Io => "operating-system operation failed",
            Self::InvalidShortcut => "invalid shortcut plan",
            Self::InvalidDialog => "invalid dialog request",
            Self::InvalidSelection => "dialog selection is invalid",
        })
    }
}
impl std::error::Error for AdapterError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DialogKind {
    File,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogRequest {
    pub kind: DialogKind,
    pub title: String,
    pub default_path: Option<PathBuf>,
    pub filters: Vec<DialogFilter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl DialogFilter {
    pub fn new(
        name: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, AdapterError> {
        let name = name.into();
        let extensions = extensions
            .into_iter()
            .map(Into::into)
            .collect::<Vec<String>>();
        if name.is_empty()
            || name.len() > 80
            || name.chars().any(char::is_control)
            || extensions.is_empty()
            || extensions.len() > 16
            || extensions.iter().any(|extension| {
                !(1..=12).contains(&extension.len())
                    || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err(AdapterError::InvalidDialog);
        }
        Ok(Self { name, extensions })
    }
}

impl DialogRequest {
    pub fn file(title: impl Into<String>) -> Self {
        Self {
            kind: DialogKind::File,
            title: title.into(),
            default_path: None,
            filters: Vec::new(),
        }
    }
    pub fn folder(title: impl Into<String>) -> Self {
        Self {
            kind: DialogKind::Folder,
            title: title.into(),
            default_path: None,
            filters: Vec::new(),
        }
    }

    pub fn filter(mut self, filter: DialogFilter) -> Self {
        self.filters.push(filter);
        self
    }
}

/// The legacy renderer contract is a path string on success and null on cancel.
pub fn legacy_path_result(path: Option<PathBuf>) -> Option<String> {
    path.map(|p| p.to_string_lossy().into_owned())
}

/// Re-checks a native-dialog selection before it is consumed by an import.
/// A picker filter is presentation only and is not a security boundary.
pub fn validate_dialog_selection(
    request: &DialogRequest,
    selected: impl AsRef<Path>,
) -> Result<PathBuf, AdapterError> {
    let selected = fs::canonicalize(selected).map_err(|_| AdapterError::InvalidSelection)?;
    let metadata = fs::symlink_metadata(&selected).map_err(|_| AdapterError::InvalidSelection)?;
    let expected_type = match request.kind {
        DialogKind::File => metadata.is_file() && !metadata.file_type().is_symlink(),
        DialogKind::Folder => metadata.is_dir() && !metadata.file_type().is_symlink(),
    };
    if !expected_type {
        return Err(AdapterError::InvalidSelection);
    }
    if request.kind == DialogKind::File && !request.filters.is_empty() {
        let extension = selected
            .extension()
            .and_then(|value| value.to_str())
            .ok_or(AdapterError::InvalidSelection)?;
        if !request.filters.iter().any(|filter| {
            filter
                .extensions
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        }) {
            return Err(AdapterError::InvalidSelection);
        }
    }
    Ok(selected)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeImportCancel {
    pub created: bool,
    pub canceled: bool,
    pub stage: &'static str,
}

impl ThemeImportCancel {
    pub const fn at(stage: &'static str) -> Self {
        Self {
            created: false,
            canceled: true,
            stage,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolChoiceResult {
    pub configured: bool,
    pub executable_name: Option<String>,
    pub canceled: bool,
}

pub fn tool_choice_result(path: Option<&Path>) -> ToolChoiceResult {
    ToolChoiceResult {
        configured: path.is_some(),
        executable_name: path.and_then(|value| {
            value
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }),
        canceled: path.is_none(),
    }
}

/// Validates an executable selected by the native picker. It cannot be fed a renderer path.
pub fn validate_windows_executable(path: impl AsRef<Path>) -> Result<PathBuf, AdapterError> {
    let request = DialogRequest::file("Choose executable")
        .filter(DialogFilter::new("Windows executable", ["exe"])?);
    validate_dialog_selection(&request, path)
}

pub fn validate_https_external(raw: &str, allowed_hosts: &[&str]) -> Result<Url, AdapterError> {
    if raw.is_empty()
        || raw.len() > MAX_URL_BYTES
        || raw.chars().any(|c| c.is_control() || c == '#')
    {
        return Err(AdapterError::InvalidUrl);
    }
    let url = Url::parse(raw).map_err(|_| AdapterError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(AdapterError::InvalidUrl);
    }
    let host = url.host_str().ok_or(AdapterError::InvalidUrl)?;
    if !allowed_hosts
        .iter()
        .any(|candidate| host.eq_ignore_ascii_case(candidate))
    {
        return Err(AdapterError::HostNotAllowed);
    }
    Ok(url)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedFolder(PathBuf);

impl ValidatedFolder {
    /// Only backend-derived paths should be passed here; renderer strings must not be accepted.
    pub fn from_backend(
        path: impl AsRef<Path>,
        approved_roots: &[PathBuf],
    ) -> Result<Self, AdapterError> {
        let path = fs::canonicalize(path).map_err(|_| AdapterError::InvalidFolder)?;
        let metadata = fs::metadata(&path).map_err(|_| AdapterError::InvalidFolder)?;
        if !metadata.is_dir() {
            return Err(AdapterError::InvalidFolder);
        }
        let approved = approved_roots
            .iter()
            .filter_map(|root| fs::canonicalize(root).ok())
            .any(|root| path.starts_with(root));
        if !approved {
            return Err(AdapterError::FolderNotAllowed);
        }
        Ok(Self(path))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleAction {
    Restart,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WindowMode {
    Normal,
    Controller,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutPlan {
    pub name: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
}

impl ShortcutPlan {
    pub fn new(
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
    ) -> Result<Self, AdapterError> {
        let name = name.into();
        let executable = executable.into();
        if name.is_empty()
            || name.len() > 128
            || name == "."
            || name == ".."
            || name
                .chars()
                .any(|value| value.is_control() || r#"<>:"/\|?*"#.contains(value))
            || name.ends_with(' ')
            || name.ends_with('.')
            || executable.as_os_str().is_empty()
            || !executable.is_absolute()
        {
            return Err(AdapterError::InvalidShortcut);
        }
        Ok(Self {
            name,
            executable,
            arguments: Vec::new(),
            working_directory: None,
        })
    }
    pub fn argument(mut self, argument: impl Into<String>) -> Result<Self, AdapterError> {
        let argument = argument.into();
        if argument.contains('\0') || argument.len() > 4096 {
            return Err(AdapterError::InvalidShortcut);
        }
        self.arguments.push(argument);
        Ok(self)
    }
}

pub trait DialogBackend {
    fn pick(&self, request: &DialogRequest) -> Result<Option<PathBuf>, AdapterError>;
}
pub trait ChoiceBackend {
    /// Returns the selected zero-based index, or `None` when cancelled.
    fn choose(
        &self,
        title: &str,
        message: &str,
        choices: &[String],
    ) -> Result<Option<usize>, AdapterError>;
}
pub trait ExternalOpener {
    fn open(&self, url: &Url) -> Result<(), AdapterError>;
}
pub trait FolderRevealer {
    fn reveal(&self, folder: &ValidatedFolder) -> Result<(), AdapterError>;
}
pub trait LifecycleBackend {
    fn apply(&self, action: LifecycleAction) -> Result<(), AdapterError>;
    fn set_window_mode(&self, mode: WindowMode) -> Result<(), AdapterError>;
}
pub trait ShortcutBackend {
    fn install(&self, plan: &ShortcutPlan) -> Result<(), AdapterError>;
}

#[cfg(feature = "tauri-adapter")]
pub mod tauri_adapter {
    //! Native implementation backed only by the official Tauri dialog plugin.
    use super::{
        validate_dialog_selection, AdapterError, ChoiceBackend, DialogBackend, DialogKind,
        DialogRequest,
    };
    use std::path::PathBuf;
    use tauri::{Manager, Runtime};
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};

    pub struct TauriDialogBackend<'a, R: Runtime, M: Manager<R>> {
        manager: &'a M,
        _runtime: std::marker::PhantomData<R>,
    }

    impl<'a, R: Runtime, M: Manager<R>> TauriDialogBackend<'a, R, M> {
        pub fn new(manager: &'a M) -> Self {
            Self {
                manager,
                _runtime: std::marker::PhantomData,
            }
        }
    }

    impl<R: Runtime, M: Manager<R>> DialogBackend for TauriDialogBackend<'_, R, M> {
        fn pick(&self, request: &DialogRequest) -> Result<Option<PathBuf>, AdapterError> {
            let mut builder = self.manager.dialog().file().set_title(&request.title);
            if let Some(default_path) = &request.default_path {
                builder = builder.set_directory(default_path);
            }
            for filter in &request.filters {
                let extensions = filter
                    .extensions
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                builder = builder.add_filter(&filter.name, &extensions);
            }
            let selected = match request.kind {
                DialogKind::File => builder.blocking_pick_file(),
                DialogKind::Folder => builder.blocking_pick_folder(),
            };
            selected
                .map(|path| {
                    path.into_path()
                        .map_err(|_| AdapterError::InvalidSelection)
                        .and_then(|path| validate_dialog_selection(request, path))
                })
                .transpose()
        }
    }

    impl<R: Runtime, M: Manager<R>> ChoiceBackend for TauriDialogBackend<'_, R, M> {
        fn choose(
            &self,
            title: &str,
            message: &str,
            choices: &[String],
        ) -> Result<Option<usize>, AdapterError> {
            if choices.is_empty() || choices.len() > 256 {
                return Err(AdapterError::InvalidDialog);
            }
            // The official plugin exposes at most three message buttons. Walk the list
            // with an explicit Next action while retaining native ownership and cancel.
            for (index, choice) in choices.iter().enumerate() {
                let result = self
                    .manager
                    .dialog()
                    .message(message)
                    .title(title)
                    .buttons(MessageDialogButtons::YesNoCancelCustom(
                        choice.clone(),
                        "Next".to_owned(),
                        "Cancel".to_owned(),
                    ))
                    .blocking_show_with_result();
                match result {
                    MessageDialogResult::Yes => return Ok(Some(index)),
                    MessageDialogResult::Custom(ref value) if value == choice => {
                        return Ok(Some(index));
                    }
                    MessageDialogResult::Cancel => return Ok(None),
                    _ => {}
                }
            }
            Ok(None)
        }
    }

    pub use tauri;
    pub use tauri_plugin_dialog;
    pub use tauri_plugin_opener;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn url_policy_rejects_non_https_credentials_ports_and_fragments() {
        assert!(validate_https_external("https://gamebanana.com/a", &["gamebanana.com"]).is_ok());
        for url in [
            "http://gamebanana.com",
            "https://evil.example",
            "https://gamebanana.com:444",
            "https://u:p@gamebanana.com",
            "https://gamebanana.com/a#x",
        ] {
            assert!(
                validate_https_external(url, &["gamebanana.com"]).is_err(),
                "{url}"
            );
        }
    }
    #[test]
    fn legacy_cancel_is_null_and_success_is_string() {
        assert_eq!(legacy_path_result(None), None);
        assert_eq!(
            legacy_path_result(Some(PathBuf::from("C:\\Delta"))),
            Some("C:\\Delta".into())
        );
    }
    #[test]
    fn folder_must_be_canonical_child_of_approved_root() {
        let root = std::env::temp_dir().join(format!("deltamod-adapter-{}", std::process::id()));
        let child = root.join("approved");
        fs::create_dir_all(&child).unwrap();
        assert!(ValidatedFolder::from_backend(&child, std::slice::from_ref(&root)).is_ok());
        assert!(
            ValidatedFolder::from_backend(std::env::temp_dir(), std::slice::from_ref(&child))
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shortcut_is_data_only_and_rejects_relative_executable() {
        assert!(ShortcutPlan::new("Deltamod", "run.exe").is_err());
        assert!(
            ShortcutPlan::new("../Deltamod", PathBuf::from("C:\\Deltamod\\Deltamod.exe")).is_err()
        );
        let plan = ShortcutPlan::new("Deltamod", PathBuf::from("C:\\Deltamod\\Deltamod.exe"))
            .unwrap()
            .argument("--controller")
            .unwrap();
        assert_eq!(plan.arguments, vec!["--controller"]);
    }

    #[test]
    fn dialog_filters_reject_renderer_metacharacters() {
        assert!(DialogFilter::new("Images", ["png", "webp"]).is_ok());
        assert!(DialogFilter::new("Images", ["*.png"]).is_err());
        assert!(DialogFilter::new("bad\nname", ["png"]).is_err());
    }

    #[test]
    fn import_selection_is_rechecked_after_the_picker() {
        let root = std::env::temp_dir().join(format!("deltamod-dialog-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let image = root.join("background.png");
        let text = root.join("background.txt");
        fs::write(&image, b"png").unwrap();
        fs::write(&text, b"text").unwrap();
        let request =
            DialogRequest::file("Image").filter(DialogFilter::new("Images", ["png"]).unwrap());
        assert_eq!(
            validate_dialog_selection(&request, &image).unwrap(),
            image.canonicalize().unwrap()
        );
        assert!(validate_dialog_selection(&request, text).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tool_choice_preserves_legacy_cancel_shape() {
        assert_eq!(
            tool_choice_result(None),
            ToolChoiceResult {
                configured: false,
                executable_name: None,
                canceled: true,
            }
        );
    }
}
