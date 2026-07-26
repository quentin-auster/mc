use std::{collections::BTreeMap, error::Error, path::PathBuf};

use mc_domain::RunId;
use thiserror::Error;

use crate::SandboxResult;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CommandToolKind {
    Command,
    Test,
    Formatter,
    Linter,
    TypeChecker,
    Build,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandToolRequest {
    pub kind: CommandToolKind,
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandToolResult {
    pub kind: CommandToolKind,
    pub command: Vec<String>,
    pub sandbox: SandboxResult,
}

#[derive(Debug, Error)]
pub enum CommandToolError {
    #[error("{kind:?} command is not allowlisted")]
    Denied { kind: CommandToolKind },
    #[error("command tool failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait CommandTools: Send + Sync {
    fn execute(
        &self,
        run_id: RunId,
        worktree: PathBuf,
        request: CommandToolRequest,
    ) -> impl Future<Output = Result<CommandToolResult, CommandToolError>> + Send;
}
