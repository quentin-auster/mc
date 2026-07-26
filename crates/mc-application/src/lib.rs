#![doc = "Application use cases and infrastructure-neutral ports for MC."]

mod event_store;
mod projection;

pub use event_store::*;
pub use mc_domain::DOMAIN_CONTRACT_VERSION;
pub use projection::*;
