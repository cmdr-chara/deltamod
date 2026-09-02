use serde::Serialize;
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionPhase {
    AcquireLock,
    ValidateInput,
    BindIntent,
    Stage,
    Verify,
    PersistHandoff,
    ReadHandoff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    Busy,
    InvalidHostRoot,
    RootNotPrivate,
    RootChanged,
    UnsupportedFilesystem,
    InvalidRequest,
    IdempotencyConflict,
    IncompleteStaging,
    InvalidPlan,
    UnsafeSource,
    NamedStreams,
    SourceHardlinked,
    SourceTooLarge,
    SourceChanged,
    InvalidStagedPackage,
    JsonContractMismatch,
    ProvenanceMismatch,
    Cancelled,
    Io,
}

impl ErrorCode {
    const fn message(self) -> &'static str {
        match self {
            Self::Busy => "theme recipe staging is busy",
            Self::InvalidHostRoot => "the trusted per-operation staging root is invalid",
            Self::RootNotPrivate => "the per-operation staging root is not private",
            Self::RootChanged => "a validated filesystem root changed",
            Self::UnsupportedFilesystem => {
                "the filesystem lacks a required safe handle-relative primitive"
            }
            Self::InvalidRequest => "the staging operation identity is invalid",
            Self::IdempotencyConflict => "the staging operation conflicts with prior intent",
            Self::IncompleteStaging => "the staging root contains untrusted or incomplete state",
            Self::InvalidPlan => "the accepted execution plan is not closed",
            Self::UnsafeSource => "a selected source is not a safe regular file",
            Self::NamedStreams => "a selected filesystem node contains a forbidden named stream",
            Self::SourceHardlinked => "a selected source has multiple filesystem links",
            Self::SourceTooLarge => "a selected source exceeds the byte limit",
            Self::SourceChanged => "a selected source changed after planning",
            Self::InvalidStagedPackage => "the staged theme package is invalid",
            Self::JsonContractMismatch => "generated JSON failed a runtime contract check",
            Self::ProvenanceMismatch => "theme provenance is invalid",
            Self::Cancelled => "theme recipe staging was cancelled at a safe checkpoint",
            Self::Io => "a local staging filesystem operation failed",
        }
    }
}

/// Bounded, path-free failure information for future host-side mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReport {
    pub code: ErrorCode,
    pub phase: ExecutionPhase,
}

impl ErrorReport {
    pub(crate) const fn new(code: ErrorCode, phase: ExecutionPhase) -> Self {
        Self { code, phase }
    }
}

impl fmt::Display for ErrorReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.message())
    }
}

impl Error for ErrorReport {}

pub(crate) type Result<T> = std::result::Result<T, ErrorReport>;

pub(crate) fn io_failure<T>(result: std::io::Result<T>, phase: ExecutionPhase) -> Result<T> {
    result.map_err(|_| ErrorReport::new(ErrorCode::Io, phase))
}
