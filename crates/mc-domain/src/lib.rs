#![doc = "Provider-neutral domain models and invariants for MC."]

/// Identifies the current version of MC's public domain contract.
pub const DOMAIN_CONTRACT_VERSION: u16 = 1;

#[cfg(test)]
mod tests {
    use super::DOMAIN_CONTRACT_VERSION;

    #[test]
    fn domain_contract_starts_at_version_one() {
        assert_eq!(DOMAIN_CONTRACT_VERSION, 1);
    }
}
