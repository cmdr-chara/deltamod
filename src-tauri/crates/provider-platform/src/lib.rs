#![forbid(unsafe_code)]
#![warn(clippy::all)]

//! Pure, fixture-driven normalization for the providers already supported by Deltamod.
//!
//! This crate deliberately performs no network or filesystem I/O. Runtime adapters supply
//! provider payloads and receive validated product contracts, stable cache identities, and
//! normalized metadata. Transient download URLs are represented by a non-serializable type.

mod capabilities;
mod digest;
mod error;
mod legacy;
mod model;
mod progress;
mod text_policy;
mod url_policy;

pub mod gamebanana;
pub mod gamejolt;
pub mod itch;
pub mod local;
pub mod moddb;
pub mod nexus;

pub use capabilities::*;
pub use error::*;
pub use legacy::*;
pub use model::*;
pub use progress::*;
pub use url_policy::EphemeralDownloadUrl;
