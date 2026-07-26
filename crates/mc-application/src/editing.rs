use std::{error::Error, ops::Range, path::PathBuf};

use bytes::Bytes;
use mc_domain::{ArtifactId, EventId, RunId};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditOperation {
    ApplyPatch,
    ReplaceRange,
    Create,
    Delete,
    Revert,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditResult {
    pub operation: EditOperation,
    pub path: PathBuf,
    pub before_artifact_id: ArtifactId,
    pub after_artifact_id: ArtifactId,
    pub event_id: EventId,
}

#[derive(Debug, Error)]
pub enum EditError {
    #[error("edit path is outside the managed worktree")]
    InvalidPath,
    #[error("invalid line range {start}..{end}")]
    InvalidRange { start: u64, end: u64 },
    #[error("file already exists")]
    AlreadyExists,
    #[error("edit failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait RepositoryEditor: Send + Sync {
    fn apply_patch(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        patch: Bytes,
    ) -> impl Future<Output = Result<EditResult, EditError>> + Send;

    fn replace_lines(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
        replacement: Bytes,
    ) -> impl Future<Output = Result<EditResult, EditError>> + Send;

    fn create(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
        contents: Bytes,
    ) -> impl Future<Output = Result<EditResult, EditError>> + Send;

    fn delete(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
    ) -> impl Future<Output = Result<EditResult, EditError>> + Send;

    fn revert(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        path: PathBuf,
    ) -> impl Future<Output = Result<EditResult, EditError>> + Send;
}
