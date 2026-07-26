use std::{collections::BTreeMap, error::Error, path::PathBuf, time::Duration};

use mc_domain::{ArtifactId, RunId};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxRequest {
    pub run_id: RunId,
    pub worktree: PathBuf,
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration: Duration,
    pub stdout_artifact_id: ArtifactId,
    pub stderr_artifact_id: ArtifactId,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox command must not be empty")]
    EmptyCommand,
    #[error("worktree path is outside the managed root")]
    InvalidWorktree,
    #[error("environment variable {name} is not allowlisted")]
    EnvironmentDenied { name: String },
    #[error("sandbox operation failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait Sandbox: Send + Sync {
    fn execute(
        &self,
        request: SandboxRequest,
    ) -> impl Future<Output = Result<SandboxResult, SandboxError>> + Send;
}
