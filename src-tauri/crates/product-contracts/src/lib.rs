#![forbid(unsafe_code)]
#![warn(clippy::all)]

//! Stable, shell-independent contracts shared by lifecycle, provider, and UI adapters.
//! Filesystem mutation belongs to runtime crates; this crate defines validated plans and
//! deterministic decisions that every runtime must honor.

pub mod fixtures;
pub mod lifecycle;
pub mod operation;
pub mod path_boundary;
pub mod provider;
pub mod retention;
pub mod schema;

pub use lifecycle::*;
pub use operation::*;
pub use path_boundary::*;
pub use provider::*;
pub use retention::*;
pub use schema::*;

pub const PRODUCT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 3;
pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const DEFAULT_RECOVERY_LIMIT_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_RECOVERY_GENERATIONS_PER_INSTALLATION: usize = 3;
pub const MINIMUM_FREE_SPACE_RESERVE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_OPERATION_HISTORY_ITEMS: usize = 100;
pub const MAX_OPERATION_HISTORY_AGE_DAYS: u32 = 30;
