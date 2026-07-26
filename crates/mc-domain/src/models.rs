use serde::{Deserialize, Serialize};

use crate::{
    ArtifactId, ClaimId, ContextItemId, ContextSnapshotId, DomainError, EventId, EvidenceId,
    InvocationId, RepositoryId, RunId, Sequence, Sha256Digest, SnapshotId, TaskId, Timestamp,
    ToolCallId, Version, require_text,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    pub id: RepositoryId,
    pub locator: String,
}

impl Repository {
    pub fn new(id: RepositoryId, locator: impl Into<String>) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            locator: require_text(locator, "repository locator")?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositorySnapshot {
    pub id: SnapshotId,
    pub repository_id: RepositoryId,
    pub parent_id: Option<SnapshotId>,
    pub source_revision: Option<String>,
    pub tree_digest: Sha256Digest,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub repository_id: RepositoryId,
    pub contract: TaskContract,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskContract {
    pub version: Version,
    pub objective: String,
    pub constraints: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

impl TaskContract {
    pub fn new(version: Version, objective: impl Into<String>) -> Result<Self, DomainError> {
        Ok(Self {
            version,
            objective: require_text(objective, "task objective")?,
            constraints: Vec::new(),
            acceptance_criteria: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub task_id: TaskId,
    pub snapshot_id: SnapshotId,
    pub status: RunStatus,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    AwaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub id: EventId,
    pub run_id: RunId,
    pub sequence: Sequence,
    pub occurred_at: Timestamp,
    pub causation_id: Option<EventId>,
    pub correlation_id: Option<String>,
    pub payload: EventPayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventPayload {
    pub kind: String,
    pub version: Version,
    pub data: serde_json::Value,
}

impl EventPayload {
    pub fn new(
        kind: impl Into<String>,
        version: Version,
        data: serde_json::Value,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            kind: require_text(kind, "event kind")?,
            version,
            data,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub run_id: RunId,
    pub requested_by: EventId,
    pub tool_name: String,
    pub arguments_artifact_id: ArtifactId,
    pub result_artifact_id: Option<ArtifactId>,
    pub status: ToolCallStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    AwaitingApproval,
    Running,
    Succeeded,
    Failed,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub run_id: RunId,
    pub kind: ArtifactKind,
    pub digest: Sha256Digest,
    pub media_type: String,
    pub byte_length: u64,
    pub object_key: String,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ModelPayload,
    ToolArguments,
    ToolResult,
    Patch,
    Log,
    RepositoryBundle,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelInvocation {
    pub id: InvocationId,
    pub run_id: RunId,
    pub context_snapshot_id: ContextSnapshotId,
    pub provider: String,
    pub model: String,
    pub request_artifact_id: ArtifactId,
    pub response_artifact_id: Option<ArtifactId>,
    pub status: ModelInvocationStatus,
    pub usage: Option<ModelTokenUsage>,
    pub latency_milliseconds: Option<u64>,
    pub provider_request_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelInvocationStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub id: ContextSnapshotId,
    pub run_id: RunId,
    pub policy_version: Version,
    pub rendered_digest: Sha256Digest,
    pub estimated_tokens: u64,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: ContextItemId,
    pub context_snapshot_id: ContextSnapshotId,
    pub sequence: Sequence,
    pub kind: ContextItemKind,
    pub source: EvidenceSource,
    pub inclusion_reason: String,
    pub estimated_tokens: u64,
    pub rendered_digest: Sha256Digest,
    pub compacted_from: Vec<ContextItemId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemKind {
    Instruction,
    TaskContract,
    Conversation,
    RepositoryEvidence,
    ToolResult,
    MemoryClaim,
    Summary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSource {
    SnapshotPath {
        snapshot_id: SnapshotId,
        path: String,
        content_digest: Sha256Digest,
    },
    Artifact {
        artifact_id: ArtifactId,
        digest: Sha256Digest,
    },
    Event {
        event_id: EventId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub scope: ClaimScope,
    pub statement: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub status: ClaimStatus,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimScope {
    Repository { repository_id: RepositoryId },
    Snapshot { snapshot_id: SnapshotId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Candidate,
    Accepted,
    Superseded,
    Invalidated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EvidenceId,
    pub claim_id: ClaimId,
    pub source: EvidenceSource,
    pub observed_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::{EventPayload, Repository, TaskContract};
    use crate::{DomainError, RepositoryId, Version};

    #[test]
    fn repository_requires_a_locator() {
        assert_eq!(
            Repository::new(RepositoryId::new(), "  "),
            Err(DomainError::EmptyField {
                field: "repository locator"
            })
        );
    }

    #[test]
    fn task_contract_requires_an_objective() {
        assert_eq!(
            TaskContract::new(Version::new(1).unwrap(), ""),
            Err(DomainError::EmptyField {
                field: "task objective"
            })
        );
    }

    #[test]
    fn event_payload_round_trips_with_its_version() {
        let payload = EventPayload::new(
            "run_started",
            Version::new(1).unwrap(),
            serde_json::json!({"source": "api"}),
        )
        .unwrap();

        let encoded = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            serde_json::from_str::<EventPayload>(&encoded).unwrap(),
            payload
        );
    }
}
