use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

domain_id!(RepositoryId);
domain_id!(SnapshotId);
domain_id!(TaskId);
domain_id!(RunId);
domain_id!(EventId);
domain_id!(ToolCallId);
domain_id!(ArtifactId);
domain_id!(InvocationId);
domain_id!(ContextSnapshotId);
domain_id!(ContextItemId);
domain_id!(ClaimId);
domain_id!(EvidenceId);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::RunId;

    #[test]
    fn identifiers_round_trip_through_text() {
        let id = RunId::new();

        assert_eq!(RunId::from_str(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn identifiers_reject_invalid_text() {
        assert!(RunId::from_str("not-a-uuid").is_err());
    }
}
