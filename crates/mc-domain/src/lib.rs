#![doc = "Provider-neutral domain models and invariants for MC."]

mod ids;
mod models;
mod primitives;

pub use ids::*;
pub use models::*;
pub use primitives::*;

/// Identifies the current version of MC's public domain contract.
pub const DOMAIN_CONTRACT_VERSION: u16 = 1;
