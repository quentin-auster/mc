use std::{error::Error, ops::Range, path::PathBuf};

use bytes::Bytes;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextMatch {
    pub path: PathBuf,
    pub line: u64,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommitSummary {
    pub commit: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitBlameLine {
    pub line: u64,
    pub commit: String,
    pub author: String,
    pub text: String,
}

#[derive(Debug, Error)]
pub enum RepositoryReadError {
    #[error("repository path is outside the managed worktree")]
    InvalidPath,
    #[error("invalid line range {start}..{end}")]
    InvalidRange { start: u64, end: u64 },
    #[error("repository read failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait RepositoryReader: Send + Sync {
    fn list_files(
        &self,
        worktree: PathBuf,
        path: PathBuf,
    ) -> impl Future<Output = Result<Vec<PathBuf>, RepositoryReadError>> + Send;

    fn read_lines(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
    ) -> impl Future<Output = Result<String, RepositoryReadError>> + Send;

    fn search_exact(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        needle: String,
    ) -> impl Future<Output = Result<Vec<TextMatch>, RepositoryReadError>> + Send;

    fn diff(
        &self,
        worktree: PathBuf,
    ) -> impl Future<Output = Result<Bytes, RepositoryReadError>> + Send;

    fn history(
        &self,
        worktree: PathBuf,
        path: Option<PathBuf>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<GitCommitSummary>, RepositoryReadError>> + Send;

    fn blame(
        &self,
        worktree: PathBuf,
        path: PathBuf,
        range: Range<u64>,
    ) -> impl Future<Output = Result<Vec<GitBlameLine>, RepositoryReadError>> + Send;
}
