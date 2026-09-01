mod error;
mod fs_guard;
mod staging;

pub use error::{ErrorCode, ErrorReport, ExecutionPhase};
pub use staging::{
    PublicationHandoff, PublicationState, StagedOutput, PUBLICATION_HANDOFF_FILE,
    STAGING_INTENT_FILE,
};

use self::{
    error::Result,
    fs_guard::{FileIdentity, GuardedRoot},
};
use deltamod_theme_recipes::{ExecutionPlan, OutputArtifact};
use std::{
    collections::BTreeSet,
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};

/// A trusted host-selected private directory dedicated to exactly one staging operation.
///
/// This type has no serde representation and exposes no path. The host must create the directory
/// with current-user-only permissions before validation. The executor accepts either an empty
/// directory or immutable state bound by its staging intent; it never accepts a theme-tree root.
pub struct HostSelectedStagingRoot {
    pub(crate) root: GuardedRoot,
}

impl fmt::Debug for HostSelectedStagingRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostSelectedStagingRoot")
            .field("path", &"<redacted>")
            .field("filesystem_identity", &"<retained-handle>")
            .finish()
    }
}

impl HostSelectedStagingRoot {
    pub fn validate(path: &Path) -> std::result::Result<Self, ErrorReport> {
        let phase = ExecutionPhase::ValidateInput;
        let root = GuardedRoot::open_private_operation(path, phase)?;
        root.check(phase)?;
        Ok(Self { root })
    }
}

/// Stable host-provided identity for one staging operation.
#[derive(Clone, Eq, PartialEq)]
pub struct StagingRequest {
    operation_id: String,
    idempotency_key: String,
}

impl fmt::Debug for StagingRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StagingRequest")
            .field("operation_id", &self.operation_id)
            .field("idempotency_key", &"<redacted>")
            .finish()
    }
}

impl StagingRequest {
    pub fn new(
        operation_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> std::result::Result<Self, ErrorReport> {
        let request = Self {
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
        };
        if !valid_identity(&request.operation_id) || !valid_identity(&request.idempotency_key) {
            return Err(ErrorReport::new(
                ErrorCode::InvalidRequest,
                ExecutionPhase::ValidateInput,
            ));
        }
        Ok(request)
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Cooperative cancellation observed only at create-only staging checkpoints.
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl fmt::Debug for CancellationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct LocalThemeRecipeExecutor {
    roots: HostSelectedStagingRoot,
    #[cfg(test)]
    test_control: Option<Arc<TestControl>>,
}

impl LocalThemeRecipeExecutor {
    #[must_use]
    pub fn new(roots: HostSelectedStagingRoot) -> Self {
        Self {
            roots,
            #[cfg(test)]
            test_control: None,
        }
    }

    /// Stage and verify one closed recipe plan.
    ///
    /// A successful result is only a publication handoff. This method has no capability for the
    /// live theme tree and cannot claim that publication occurred.
    pub fn execute(
        &self,
        request: &StagingRequest,
        plan: &ExecutionPlan,
        cancellation: &CancellationToken,
    ) -> std::result::Result<PublicationHandoff, ErrorReport> {
        self.roots.root.check(ExecutionPhase::AcquireLock)?;
        let _operation_guard = acquire_operation_lock(self.roots.root.identity())?;
        staging::execute(self, request, plan, cancellation)
    }

    pub(crate) fn root(&self) -> &GuardedRoot {
        &self.roots.root
    }

    pub(crate) fn checkpoint(
        &self,
        checkpoint: Checkpoint,
        phase: ExecutionPhase,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        #[cfg(test)]
        if let Some(control) = &self.test_control {
            (control.callback)(checkpoint);
        }
        #[cfg(not(test))]
        let _ = checkpoint;
        if cancellation.is_cancelled() {
            Err(ErrorReport::new(ErrorCode::Cancelled, phase))
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn with_test_control(
        roots: HostSelectedStagingRoot,
        callback: impl Fn(Checkpoint) + Send + Sync + 'static,
    ) -> Self {
        Self {
            roots,
            test_control: Some(Arc::new(TestControl {
                callback: Box::new(callback),
            })),
        }
    }
}

static ACTIVE_OPERATION_ROOTS: OnceLock<Mutex<BTreeSet<FileIdentity>>> = OnceLock::new();

struct OperationGuard {
    identity: FileIdentity,
}

fn acquire_operation_lock(identity: FileIdentity) -> Result<OperationGuard> {
    let active = ACTIVE_OPERATION_ROOTS.get_or_init(|| Mutex::new(BTreeSet::new()));
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !active.insert(identity) {
        return Err(ErrorReport::new(
            ErrorCode::Busy,
            ExecutionPhase::AcquireLock,
        ));
    }
    Ok(OperationGuard { identity })
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        let Some(active) = ACTIVE_OPERATION_ROOTS.get() else {
            return;
        };
        let mut active = active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        active.remove(&self.identity);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Checkpoint {
    BeforeIntent,
    AfterIntent,
    BeforeOutput(OutputArtifact),
    BeforeVerification,
    BeforeHandoff,
    LockHeld,
}

#[cfg(test)]
struct TestControl {
    callback: Box<dyn Fn(Checkpoint) + Send + Sync>,
}

#[cfg(test)]
mod tests;
