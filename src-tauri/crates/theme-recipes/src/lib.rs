#![forbid(unsafe_code)]
#![warn(clippy::all)]

//! Strict, local-only theme recipe definitions and execution plans.
//!
//! This crate deliberately stops before filesystem mutation. It validates exact-file
//! selectors beneath a user-selected game root, then emits a closed plan that a runtime
//! must execute with the stated no-follow policy. The built-in recipes remain disabled
//! until installation-specific selectors have been validated.

mod definitions;
mod path_policy;
mod plan;
mod provenance;
mod schema;

pub use definitions::{
    definition, definitions, parse_and_validate_recipe, validate_recipe, ValidatedRecipe,
};
pub use path_policy::{
    SelectedGameRoot, SelectorCandidate, Sha256Digest, SourceIdentifier, ValidatedSelector,
    ValidatedSelectors, MAX_SOURCE_BYTES,
};
pub use plan::{
    build_execution_plan, ExecutionPlan, OutputArtifact, PlannedOutput, PlannedRead, ReadPolicy,
};
pub use provenance::{
    build_provenance, provenance_json, provenance_sha256, OutputProvenance, ProvenanceDocument,
    ProvenanceSource, ProvenanceTransforms, RightsMarker,
};
pub use schema::{
    Attribution, AudioFormat, MediaType, OutputScope, RecipeDefinition, RecipeId, RecipeOutput,
    RecipeSelectors, RecipeTransforms, RightsAttestation, SelectorKind, SelectorPlaceholder,
    SelectorStatus, SourceSlot, ThemeManifest, Transform, TransformKind, EXTRACTOR_VERSION,
    PROVENANCE_SCHEMA_VERSION, RECIPE_SCHEMA_VERSION, THEME_MANIFEST_SCHEMA_VERSION,
};

use deltamod_product_contracts::PathBoundaryError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RecipeError>;

#[derive(Debug, Error)]
pub enum RecipeError {
    #[error("recipe JSON does not match the strict schema")]
    InvalidRecipeJson(#[source] serde_json::Error),
    #[error("unsupported recipe schema version {0}")]
    UnsupportedSchema(u32),
    #[error("recipe differs from its closed built-in definition")]
    NonCanonicalDefinition,
    #[error("a user-selected game root must be absolute")]
    GameRootMustBeAbsolute,
    #[error("selector path violates the canonical game-root boundary")]
    PathBoundary(#[from] PathBoundaryError),
    #[error("source identifier is malformed")]
    InvalidSourceIdentifier,
    #[error("SHA-256 digest is malformed")]
    InvalidSha256,
    #[error("selector set contains a duplicate slot, identifier, or path")]
    DuplicateSelector,
    #[error("selector set is missing {0:?}")]
    MissingSelector(SourceSlot),
    #[error("source selector points to an unsafe or non-regular entry")]
    UnsafeSource,
    #[error("source file could not be read with no-follow semantics")]
    SourceIo(#[source] std::io::Error),
    #[error("source exceeds the local extraction size limit")]
    SourceTooLarge,
    #[error("source media does not match the recipe's allowlisted output format")]
    SourceMediaMismatch,
    #[error("validated selectors belong to a different recipe or game root")]
    SelectorContextMismatch,
    #[error("rights attestation is required for local extraction")]
    RightsAttestationRequired,
    #[error("recipe selectors remain intentionally unresolved")]
    UnresolvedSelectors,
    #[error("a closed output path could not be constructed")]
    InvalidOutputPath,
    #[error("JSON output could not be serialized")]
    Serialization(#[source] serde_json::Error),
}
