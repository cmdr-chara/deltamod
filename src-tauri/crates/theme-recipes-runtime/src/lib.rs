#![forbid(unsafe_code)]
#![warn(clippy::all)]

//! Host-local staging boundary for accepted Deltamod theme-recipe plans.
//!
//! Production availability remains disabled. The executor is compiled only with the non-default
//! `theme-recipes-local-executor` feature and can write only to a trusted host-selected private
//! per-operation staging root. It cannot publish, replace, rename, or remove anything from the
//! application theme tree. The returned handoff is intentionally unwired: lifecycle-runtime must
//! consume it and perform any future publication under its own transaction and lease contracts.

mod metadata;

pub use metadata::{
    availability, qa_summary, staging_integration, AvailabilityMetadata, AvailabilityState, QaCard,
    SelectorResolution, StagingIntegration,
};

#[cfg(feature = "theme-recipes-local-executor")]
mod executor;

#[cfg(feature = "theme-recipes-local-executor")]
pub use executor::{
    CancellationToken, ErrorCode, ErrorReport, ExecutionPhase, HostSelectedStagingRoot,
    LocalThemeRecipeExecutor, PublicationHandoff, PublicationState, StagedOutput, StagingRequest,
    PUBLICATION_HANDOFF_FILE, STAGING_INTENT_FILE,
};
