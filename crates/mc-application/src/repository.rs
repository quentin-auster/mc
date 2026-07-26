use std::error::Error;

use mc_domain::{Repository, RepositoryId, RepositorySnapshot, SnapshotId, Timestamp};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterRepository {
    pub id: RepositoryId,
    pub locator: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRepositorySnapshot {
    pub id: SnapshotId,
    pub repository_id: RepositoryId,
    pub branch: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum RepositoryStoreError {
    #[error("repository {repository_id} does not exist")]
    NotFound { repository_id: RepositoryId },
    #[error("invalid Git branch name: {branch}")]
    InvalidBranch { branch: String },
    #[error("repository operation failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait RepositoryStore: Send + Sync {
    fn register(
        &self,
        repository: RegisterRepository,
    ) -> impl Future<Output = Result<Repository, RepositoryStoreError>> + Send;

    fn snapshot(
        &self,
        snapshot: CreateRepositorySnapshot,
    ) -> impl Future<Output = Result<RepositorySnapshot, RepositoryStoreError>> + Send;
}
