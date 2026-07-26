use std::{error::Error, fmt};

use bytes::Bytes;
use mc_domain::{
    ArtifactId, ContextSnapshotId, InvocationId, ModelInvocation, ModelTokenUsage, RunId, Timestamp,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub struct ModelProviderRequest {
    pub model: String,
    pub input: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelProviderResponse {
    pub body: Bytes,
    pub usage: ModelTokenUsage,
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelProviderFailure {
    pub status_code: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub body: Bytes,
    pub provider_request_id: Option<String>,
}

impl fmt::Display for ModelProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn invoke(
        &self,
        request: ModelProviderRequest,
    ) -> impl Future<Output = Result<ModelProviderResponse, ModelProviderFailure>> + Send;
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewModelInvocation {
    pub id: InvocationId,
    pub run_id: RunId,
    pub context_snapshot_id: ContextSnapshotId,
    pub model: String,
    pub request_artifact_id: ArtifactId,
    pub created_at: Timestamp,
}

#[derive(Debug, Error)]
pub enum ModelInvocationError {
    #[error("model provider invocation {invocation_id} failed: {failure}")]
    Provider {
        invocation_id: InvocationId,
        failure: ModelProviderFailure,
    },
    #[error("model invocation persistence failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub trait ModelInvocationService: Send + Sync {
    fn invoke(
        &self,
        invocation: NewModelInvocation,
    ) -> impl Future<Output = Result<ModelInvocation, ModelInvocationError>> + Send;
}
