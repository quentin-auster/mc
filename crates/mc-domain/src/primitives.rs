use std::num::{NonZeroU32, NonZeroU64};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("version must be greater than zero")]
    InvalidVersion,
    #[error("SHA-256 digest must contain exactly 64 lowercase hexadecimal characters")]
    InvalidSha256Digest,
    #[error("sequence must be greater than zero")]
    InvalidSequence,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(NonZeroU32);

impl Version {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(DomainError::InvalidVersion)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sequence(NonZeroU64);

impl Sequence {
    pub fn new(value: u64) -> Result<Self, DomainError> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(DomainError::InvalidSequence)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    #[must_use]
    pub const fn from_unix_milliseconds(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn unix_milliseconds(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let valid = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidSha256Digest)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn require_text(
    value: impl Into<String>,
    field: &'static str,
) -> Result<String, DomainError> {
    let value = value.into();
    if value.trim().is_empty() {
        Err(DomainError::EmptyField { field })
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{DomainError, Sequence, Sha256Digest, Version};

    #[test]
    fn versions_and_sequences_reject_zero() {
        assert_eq!(Version::new(0), Err(DomainError::InvalidVersion));
        assert_eq!(Sequence::new(0), Err(DomainError::InvalidSequence));
    }

    #[test]
    fn digest_accepts_lowercase_sha256() {
        let digest = "0123456789abcdef".repeat(4);

        assert_eq!(Sha256Digest::new(&digest).unwrap().as_str(), digest);
    }

    #[test]
    fn digest_rejects_wrong_length_or_case() {
        assert_eq!(
            Sha256Digest::new("abc"),
            Err(DomainError::InvalidSha256Digest)
        );
        assert_eq!(
            Sha256Digest::new("A".repeat(64)),
            Err(DomainError::InvalidSha256Digest)
        );
    }
}
