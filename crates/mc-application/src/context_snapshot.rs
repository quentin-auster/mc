use std::error::Error;

use bytes::Bytes;
use mc_domain::{
    ContextItemId, ContextItemKind, ContextSnapshot, ContextSnapshotId, EvidenceSource, RunId,
    Timestamp, Version,
};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedContextItem {
    pub id: ContextItemId,
    pub kind: ContextItemKind,
    pub source: EvidenceSource,
    pub inclusion_reason: String,
    pub estimated_tokens: u64,
    pub rendered: Bytes,
    pub compacted_from: Vec<ContextItemId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewContextSnapshot {
    pub id: ContextSnapshotId,
    pub run_id: RunId,
    pub policy_version: Version,
    pub items: Vec<RenderedContextItem>,
    pub created_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum ContextSnapshotError {
    #[error("context snapshot {snapshot_id} does not exist")]
    NotFound { snapshot_id: ContextSnapshotId },
    #[error("context item {item_id} has an empty inclusion reason")]
    EmptyInclusionReason { item_id: ContextItemId },
    #[error("context snapshot store failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait ContextSnapshotStore: Send + Sync {
    fn persist(
        &self,
        snapshot: NewContextSnapshot,
    ) -> impl Future<Output = Result<ContextSnapshot, ContextSnapshotError>> + Send;

    fn reconstruct(
        &self,
        snapshot_id: ContextSnapshotId,
    ) -> impl Future<Output = Result<Bytes, ContextSnapshotError>> + Send;
}
