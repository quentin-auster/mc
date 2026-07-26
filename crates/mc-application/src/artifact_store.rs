use std::{error::Error, ops::Range, pin::Pin};

use bytes::Bytes;
use futures_core::Stream;
use mc_domain::{Artifact, ArtifactId, ArtifactKind, RunId, Timestamp};
use thiserror::Error;

pub type ArtifactBody =
    Pin<Box<dyn Stream<Item = Result<Bytes, ArtifactStoreError>> + Send + 'static>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewArtifact {
    pub id: ArtifactId,
    pub run_id: RunId,
    pub kind: ArtifactKind,
    pub media_type: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    #[error("artifact {artifact_id} does not exist")]
    NotFound { artifact_id: ArtifactId },
    #[error("invalid artifact byte range {start}..{end} for length {length}")]
    InvalidRange { start: u64, end: u64, length: u64 },
    #[error("artifact store failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait ArtifactStore: Send + Sync {
    fn put(
        &self,
        artifact: NewArtifact,
        body: ArtifactBody,
    ) -> impl Future<Output = Result<Artifact, ArtifactStoreError>> + Send;

    fn read(
        &self,
        artifact_id: ArtifactId,
    ) -> impl Future<Output = Result<ArtifactBody, ArtifactStoreError>> + Send;

    fn read_range(
        &self,
        artifact_id: ArtifactId,
        range: Range<u64>,
    ) -> impl Future<Output = Result<Bytes, ArtifactStoreError>> + Send;
}
