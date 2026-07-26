use std::{error::Error, path::PathBuf};

use bytes::Bytes;
use mc_domain::{RunId, SnapshotId};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeState {
    pub run_id: RunId,
    pub snapshot_id: SnapshotId,
    pub path: PathBuf,
    pub dirty_files: Vec<String>,
    pub diff: Bytes,
    pub retained: bool,
}

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("run {run_id} does not have a managed worktree")]
    NotFound { run_id: RunId },
    #[error("worktree operation failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait WorktreeManager: Send + Sync {
    fn create(
        &self,
        run_id: RunId,
        snapshot_id: SnapshotId,
    ) -> impl Future<Output = Result<WorktreeState, WorktreeError>> + Send;

    fn inspect(
        &self,
        run_id: RunId,
    ) -> impl Future<Output = Result<WorktreeState, WorktreeError>> + Send;

    fn complete(
        &self,
        run_id: RunId,
        outcome: WorktreeOutcome,
        retain_failed: bool,
    ) -> impl Future<Output = Result<Option<WorktreeState>, WorktreeError>> + Send;
}
