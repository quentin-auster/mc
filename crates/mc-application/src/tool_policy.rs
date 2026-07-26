use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::PathBuf,
};

use mc_domain::{RunId, Timestamp, ToolPolicyDecisionId, Version};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CommandToolKind;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRole {
    Explorer,
    Editor,
    Executor,
    Reviewer,
    Administrator,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolAction {
    Read {
        path: PathBuf,
    },
    Edit {
        path: PathBuf,
    },
    Command {
        category: CommandToolKind,
        argv: Vec<String>,
    },
    Network {
        host: String,
    },
    Secret {
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RolePolicy {
    pub allow_read: bool,
    pub allow_edit: bool,
    pub path_prefixes: Vec<PathBuf>,
    pub command_prefixes: BTreeMap<CommandToolKind, Vec<Vec<String>>>,
    pub network_hosts: BTreeSet<String>,
    pub secret_names: BTreeSet<String>,
    pub max_calls: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPolicyConfig {
    pub version: Version,
    pub roles: BTreeMap<ToolRole, RolePolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPolicyRequest {
    pub id: ToolPolicyDecisionId,
    pub run_id: RunId,
    pub role: ToolRole,
    pub action: ToolAction,
    pub requested_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDenialReason {
    RoleDenied,
    PathDenied,
    CommandDenied,
    NetworkDenied,
    SecretDenied,
    BudgetExceeded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPolicyDecision {
    pub id: ToolPolicyDecisionId,
    pub run_id: RunId,
    pub role: ToolRole,
    pub action: ToolAction,
    pub allowed: bool,
    pub denial_reason: Option<PolicyDenialReason>,
    pub remaining_calls: u64,
    pub policy_version: Version,
    pub decided_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum ToolPolicyError {
    #[error("tool policy failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait ToolPolicyEngine: Send + Sync {
    fn authorize(
        &self,
        request: ToolPolicyRequest,
    ) -> impl Future<Output = Result<ToolPolicyDecision, ToolPolicyError>> + Send;
}
