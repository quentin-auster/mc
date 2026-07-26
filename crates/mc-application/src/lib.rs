#![doc = "Application use cases and infrastructure-neutral ports for MC."]

mod event_store;

pub use event_store::*;
pub use mc_domain::DOMAIN_CONTRACT_VERSION;
