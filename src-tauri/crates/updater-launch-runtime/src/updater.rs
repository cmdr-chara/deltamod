use std::{fmt, sync::Arc};

pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
pub const UPDATE_AVAILABLE_EVENT: &str = "updateAvailable";
pub const UPDATER_STATUS_EVENT: &str = "updater-status";
pub const UPDATER_PROGRESS_EVENT: &str = "updater-progress";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateInfo {
    pub update: bool,
    pub version: String,
    pub release_name: Option<String>,
}

impl UpdateInfo {
    pub fn available(
        version: impl Into<String>,
        release_name: Option<String>,
    ) -> Result<Self, UpdateError> {
        let version = version.into();
        if version.is_empty() || version.len() > 128 || version.chars().any(char::is_control) {
            return Err(UpdateError::InvalidMetadata("version"));
        }
        if release_name
            .as_ref()
            .is_some_and(|name| name.len() > 512 || name.chars().any(char::is_control))
        {
            return Err(UpdateError::InvalidMetadata("release name"));
        }
        Ok(Self {
            update: true,
            version,
            release_name,
        })
    }
}

/// An artifact downloaded and signature-verified by `tauri-plugin-updater`.
///
/// Its payload is deliberately opaque: callers cannot turn an arbitrary path or byte buffer into
/// an installable artifact. Concrete adapters should construct it only after the plugin's mandatory
/// minisign verification has completed successfully.
#[derive(Clone)]
pub struct VerifiedArtifact {
    version: String,
    payload: Arc<dyn std::any::Any + Send + Sync>,
}

impl fmt::Debug for VerifiedArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedArtifact")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl VerifiedArtifact {
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Trust boundary for an adapter backed by the official updater plugin.
    #[cfg_attr(not(any(feature = "tauri-adapter", test)), allow(dead_code))]
    pub(crate) fn from_verified_plugin_payload<T: Send + Sync + 'static>(
        version: impl Into<String>,
        payload: T,
    ) -> Self {
        Self {
            version: version.into(),
            payload: Arc::new(payload),
        }
    }

    #[cfg(feature = "tauri-adapter")]
    fn plugin_payload<T: 'static>(&self) -> Option<&T> {
        self.payload.downcast_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdaterGate {
    packaged: bool,
    updater_artifacts: bool,
    https_endpoint: bool,
    public_key: bool,
}

impl UpdaterGate {
    pub const fn disabled() -> Self {
        Self {
            packaged: false,
            updater_artifacts: false,
            https_endpoint: false,
            public_key: false,
        }
    }

    pub fn configured(
        packaged: bool,
        updater_artifacts: bool,
        endpoints: &[impl AsRef<str>],
        public_key: &str,
    ) -> Self {
        Self {
            packaged,
            updater_artifacts,
            https_endpoint: !endpoints.is_empty()
                && endpoints
                    .iter()
                    .all(|endpoint| is_https_endpoint(endpoint.as_ref())),
            public_key: !public_key.trim().is_empty(),
        }
    }

    pub const fn supported(self) -> bool {
        self.packaged && self.updater_artifacts && self.https_endpoint && self.public_key
    }

    pub const fn reason(self) -> Option<&'static str> {
        if !self.packaged {
            Some("development-build")
        } else if !self.updater_artifacts {
            Some("updater-artifacts-disabled")
        } else if !self.https_endpoint {
            Some("https-endpoint-required")
        } else if !self.public_key {
            Some("public-key-required")
        } else {
            None
        }
    }
}

fn is_https_endpoint(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !authority.starts_with('.')
        && !authority.ends_with('.')
        && authority.contains('.')
        && !endpoint.contains(['\\', '#'])
        && !endpoint.chars().any(char::is_control)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    Available(UpdateInfo),
    Downloading(UpdateInfo),
    Downloaded(VerifiedArtifact),
    Installing(VerifiedArtifact),
    Installed(String),
    Ignored(String),
    Failed(String),
    Unsupported(&'static str),
}

impl PartialEq for VerifiedArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && Arc::ptr_eq(&self.payload, &other.payload)
    }
}
impl Eq for VerifiedArtifact {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateStatus {
    pub state: &'static str,
    pub available: bool,
    pub supported: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateProgress {
    pub operation_id: &'static str,
    pub phase: &'static str,
    pub completed: u64,
    pub total: Option<u64>,
    pub percentage: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEvent {
    Available(UpdateInfo),
    Status(UpdateStatus),
    Progress(UpdateProgress),
}

pub trait UpdateEventSink {
    fn emit(&self, event: UpdateEvent);
}

impl<F: Fn(UpdateEvent)> UpdateEventSink for F {
    fn emit(&self, event: UpdateEvent) {
        self(event);
    }
}

/// Adapter boundary matching `tauri_plugin_updater::Update`: check, verified download, install.
pub trait UpdateAdapter {
    fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError>;
    fn download_and_verify(
        &mut self,
        info: &UpdateInfo,
        max_bytes: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
    ) -> Result<VerifiedArtifact, UpdateError>;
    fn install(&mut self, artifact: VerifiedArtifact) -> Result<(), UpdateError>;
}

pub struct Updater<A, E> {
    adapter: A,
    events: E,
    gate: UpdaterGate,
    state: UpdateState,
    max_artifact_bytes: u64,
    ignored_for_session: bool,
}

impl<A: UpdateAdapter, E: UpdateEventSink> Updater<A, E> {
    pub fn new(adapter: A, events: E, gate: UpdaterGate) -> Self {
        let state = gate
            .reason()
            .map_or(UpdateState::Idle, UpdateState::Unsupported);
        Self {
            adapter,
            events,
            gate,
            state,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
            ignored_for_session: false,
        }
    }

    pub fn with_max_artifact_bytes(mut self, bytes: u64) -> Result<Self, UpdateError> {
        if bytes == 0 {
            return Err(UpdateError::InvalidLimit);
        }
        self.max_artifact_bytes = bytes;
        Ok(self)
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn status(&self) -> UpdateStatus {
        let (name, available, version, reason) = match &self.state {
            UpdateState::Idle => ("idle", false, None, None),
            UpdateState::Checking => ("checking", false, None, None),
            UpdateState::Available(info) => ("available", true, Some(info.version.clone()), None),
            UpdateState::Downloading(info) => {
                ("downloading", true, Some(info.version.clone()), None)
            }
            UpdateState::Downloaded(artifact) => (
                "downloaded",
                true,
                Some(artifact.version().to_owned()),
                None,
            ),
            UpdateState::Installing(artifact) => (
                "installing",
                true,
                Some(artifact.version().to_owned()),
                None,
            ),
            UpdateState::Installed(version) => ("installed", false, Some(version.clone()), None),
            UpdateState::Ignored(version) => ("ignored", false, Some(version.clone()), None),
            UpdateState::Failed(message) => ("failed", false, None, Some(message.clone())),
            UpdateState::Unsupported(message) => {
                ("unsupported", false, None, Some((*message).to_owned()))
            }
        };
        UpdateStatus {
            state: name,
            available,
            supported: self.gate.supported(),
            version,
            reason,
        }
    }

    fn publish_status(&self) {
        self.events.emit(UpdateEvent::Status(self.status()));
    }

    /// Implements Electron's `fireUpdate`: check and emit `updateAvailable`, returning a boolean.
    pub fn fire_update(&mut self) -> Result<bool, UpdateError> {
        if let Some(reason) = self.gate.reason() {
            self.state = UpdateState::Unsupported(reason);
            self.publish_status();
            return Ok(false);
        }
        if self.ignored_for_session {
            self.publish_status();
            return Ok(false);
        }
        self.state = UpdateState::Checking;
        self.publish_status();
        match self.adapter.check() {
            Ok(Some(info)) if info.update => {
                self.state = UpdateState::Available(info.clone());
                self.events.emit(UpdateEvent::Available(info));
                self.publish_status();
                Ok(true)
            }
            Ok(_) => {
                self.state = UpdateState::Idle;
                self.publish_status();
                Ok(false)
            }
            Err(error) => {
                self.state = UpdateState::Failed(error.to_string());
                self.publish_status();
                // Electron's fireUpdate catches check failures and resolves false.
                Ok(false)
            }
        }
    }

    /// Implements Electron's `ignore-update` for the update currently offered this session.
    pub fn ignore_update(&mut self) -> Result<(), UpdateError> {
        let version = match &self.state {
            UpdateState::Available(info) => info.version.clone(),
            _ => return Err(UpdateError::InvalidTransition),
        };
        self.ignored_for_session = true;
        self.state = UpdateState::Ignored(version);
        self.publish_status();
        Ok(())
    }

    /// Implements Electron's `start-update`: bounded download, mandatory verification, then install.
    pub fn start_update(&mut self) -> Result<(), UpdateError> {
        let info = match &self.state {
            UpdateState::Available(info) => info.clone(),
            _ => return Err(UpdateError::InvalidTransition),
        };
        self.state = UpdateState::Downloading(info.clone());
        self.publish_status();
        let limit = self.max_artifact_bytes;
        let events = &self.events;
        let mut completed = 0_u64;
        let mut progress = |chunk: u64, total: Option<u64>| {
            completed = completed
                .checked_add(chunk)
                .ok_or(UpdateError::ArtifactTooLarge { limit })?;
            if completed > limit || total.is_some_and(|value| value > limit) {
                return Err(UpdateError::ArtifactTooLarge { limit });
            }
            events.emit(UpdateEvent::Progress(UpdateProgress {
                operation_id: "community-update",
                phase: "download",
                completed,
                total,
                percentage: total
                    .filter(|total| *total > 0)
                    .map(|total| completed as f64 * 100.0 / total as f64),
            }));
            Ok(())
        };
        let artifact = match self
            .adapter
            .download_and_verify(&info, limit, &mut progress)
        {
            Ok(artifact) if artifact.version() == info.version => artifact,
            Ok(_) => return self.fail(UpdateError::VersionMismatch),
            Err(error) => return self.fail(error),
        };
        self.state = UpdateState::Downloaded(artifact.clone());
        self.publish_status();
        self.state = UpdateState::Installing(artifact.clone());
        self.publish_status();
        if let Err(error) = self.adapter.install(artifact) {
            return self.fail(error);
        }
        self.state = UpdateState::Installed(info.version);
        self.publish_status();
        Ok(())
    }

    fn fail<T>(&mut self, error: UpdateError) -> Result<T, UpdateError> {
        self.state = UpdateState::Failed(error.to_string());
        self.publish_status();
        Err(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateError {
    InvalidTransition,
    InvalidMetadata(&'static str),
    InvalidLimit,
    ArtifactTooLarge { limit: u64 },
    VersionMismatch,
    SignatureVerification(String),
    Adapter(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition => f.write_str("invalid updater transition"),
            Self::InvalidMetadata(field) => write!(f, "invalid update {field}"),
            Self::InvalidLimit => f.write_str("artifact limit must be positive"),
            Self::ArtifactTooLarge { limit } => write!(f, "update artifact exceeds {limit} bytes"),
            Self::VersionMismatch => f.write_str("verified artifact version mismatch"),
            Self::SignatureVerification(message) => {
                write!(f, "update signature verification failed: {message}")
            }
            Self::Adapter(message) => write!(f, "updater adapter failed: {message}"),
        }
    }
}
impl std::error::Error for UpdateError {}

#[cfg(feature = "tauri-adapter")]
pub mod tauri_adapter {
    use super::*;

    /// Host-side bridge. Implement this with `tauri_plugin_updater::UpdaterExt`; its `Update`
    /// value must remain owned by the payload until `install` consumes it.
    pub trait OfficialUpdaterPlugin {
        type VerifiedPayload: Send + Sync + 'static;
        fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError>;
        fn download_and_verify(
            &mut self,
            info: &UpdateInfo,
            max_bytes: u64,
            progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
        ) -> Result<Self::VerifiedPayload, UpdateError>;
        fn install_verified(&mut self, payload: &Self::VerifiedPayload) -> Result<(), UpdateError>;
    }

    pub struct Adapter<P>(pub P);
    impl<P: OfficialUpdaterPlugin> UpdateAdapter for Adapter<P> {
        fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError> {
            self.0.check()
        }
        fn download_and_verify(
            &mut self,
            info: &UpdateInfo,
            max_bytes: u64,
            progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
        ) -> Result<VerifiedArtifact, UpdateError> {
            let payload = self.0.download_and_verify(info, max_bytes, progress)?;
            Ok(VerifiedArtifact::from_verified_plugin_payload(
                info.version.clone(),
                payload,
            ))
        }
        fn install(&mut self, artifact: VerifiedArtifact) -> Result<(), UpdateError> {
            let payload = artifact
                .plugin_payload::<P::VerifiedPayload>()
                .ok_or_else(|| UpdateError::Adapter("verified artifact type mismatch".into()))?;
            self.0.install_verified(payload)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeAdapter {
        install_calls: usize,
        oversize: bool,
        bad_version: bool,
    }
    impl UpdateAdapter for FakeAdapter {
        fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError> {
            Ok(Some(
                UpdateInfo::available("2.1.0", Some("Community 2.1".into())).unwrap(),
            ))
        }
        fn download_and_verify(
            &mut self,
            _: &UpdateInfo,
            _: u64,
            progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
        ) -> Result<VerifiedArtifact, UpdateError> {
            if self.oversize {
                progress(11, Some(11))?;
            } else {
                progress(4, Some(10))?;
                progress(6, Some(10))?;
            }
            Ok(VerifiedArtifact::from_verified_plugin_payload(
                if self.bad_version { "9" } else { "2.1.0" },
                vec![1_u8],
            ))
        }
        fn install(&mut self, _: VerifiedArtifact) -> Result<(), UpdateError> {
            self.install_calls += 1;
            Ok(())
        }
    }

    fn gate() -> UpdaterGate {
        UpdaterGate::configured(
            true,
            true,
            &["https://github.com/cmdr-chara/deltamod/releases/latest/download/latest.json"],
            "trusted public key",
        )
    }

    #[test]
    fn unsupported_build_is_truthful_and_never_checks() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let capture = events.clone();
        let mut updater = Updater::new(
            FakeAdapter {
                install_calls: 0,
                oversize: false,
                bad_version: false,
            },
            move |event| capture.lock().unwrap().push(event),
            UpdaterGate::disabled(),
        );
        assert!(!updater.fire_update().unwrap());
        assert_eq!(
            updater.status().reason.as_deref(),
            Some("development-build")
        );
        assert!(!updater.status().supported);
    }

    #[test]
    fn electron_lifecycle_emits_available_progress_and_installs_verified_payload() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let capture = events.clone();
        let mut updater = Updater::new(
            FakeAdapter {
                install_calls: 0,
                oversize: false,
                bad_version: false,
            },
            move |event| capture.lock().unwrap().push(event),
            gate(),
        )
        .with_max_artifact_bytes(10)
        .unwrap();
        assert!(updater.fire_update().unwrap());
        updater.start_update().unwrap();
        assert!(matches!(updater.state(), UpdateState::Installed(version) if version == "2.1.0"));
        let events = events.lock().unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, UpdateEvent::Available(_))));
        assert!(events.iter().any(|event| matches!(event, UpdateEvent::Progress(progress) if progress.completed == 10 && progress.percentage == Some(100.0))));
    }

    #[test]
    fn ignore_is_session_state_and_prevents_install() {
        let mut updater = Updater::new(
            FakeAdapter {
                install_calls: 0,
                oversize: false,
                bad_version: false,
            },
            |_| {},
            gate(),
        );
        updater.fire_update().unwrap();
        updater.ignore_update().unwrap();
        assert!(matches!(updater.state(), UpdateState::Ignored(version) if version == "2.1.0"));
        assert_eq!(updater.start_update(), Err(UpdateError::InvalidTransition));
        assert!(!updater.fire_update().unwrap());
        assert!(matches!(updater.state(), UpdateState::Ignored(_)));
    }

    #[test]
    fn bounded_download_and_version_binding_block_install() {
        let mut oversize = Updater::new(
            FakeAdapter {
                install_calls: 0,
                oversize: true,
                bad_version: false,
            },
            |_| {},
            gate(),
        )
        .with_max_artifact_bytes(10)
        .unwrap();
        oversize.fire_update().unwrap();
        assert!(matches!(
            oversize.start_update(),
            Err(UpdateError::ArtifactTooLarge { limit: 10 })
        ));
        let mut mismatch = Updater::new(
            FakeAdapter {
                install_calls: 0,
                oversize: false,
                bad_version: true,
            },
            |_| {},
            gate(),
        );
        mismatch.fire_update().unwrap();
        assert_eq!(mismatch.start_update(), Err(UpdateError::VersionMismatch));
    }

    #[test]
    fn updater_gate_rejects_non_https_or_credentialed_endpoints() {
        for endpoint in [
            "http://updates.example.com/latest.json",
            "https://user@updates.example.com/latest.json",
            "https://updates.example.com/latest.json#fragment",
        ] {
            let gate = UpdaterGate::configured(true, true, &[endpoint], "public key");
            assert!(!gate.supported());
            assert_eq!(gate.reason(), Some("https-endpoint-required"));
        }
    }

    #[test]
    fn fire_update_preserves_electron_false_on_check_failure() {
        struct FailingCheck;
        impl UpdateAdapter for FailingCheck {
            fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError> {
                Err(UpdateError::Adapter("offline".into()))
            }
            fn download_and_verify(
                &mut self,
                _: &UpdateInfo,
                _: u64,
                _: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
            ) -> Result<VerifiedArtifact, UpdateError> {
                unreachable!()
            }
            fn install(&mut self, _: VerifiedArtifact) -> Result<(), UpdateError> {
                unreachable!()
            }
        }
        let mut updater = Updater::new(FailingCheck, |_| {}, gate());
        assert!(!updater.fire_update().unwrap());
        assert!(matches!(updater.state(), UpdateState::Failed(_)));
    }
}
