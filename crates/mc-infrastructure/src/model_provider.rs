use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use mc_application::{
    ArtifactStore, ModelInvocationError, ModelInvocationService, ModelProvider,
    ModelProviderFailure, ModelProviderRequest, ModelProviderResponse, NewArtifact,
    NewModelInvocation,
};
use mc_domain::{
    ArtifactId, ArtifactKind, ModelInvocation, ModelInvocationStatus, ModelTokenUsage, Timestamp,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, instrument, warn};

#[derive(Clone)]
pub struct OpenAiResponsesProvider {
    client: Client,
    endpoint: String,
    api_key: String,
}

impl std::fmt::Debug for OpenAiResponsesProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiResponsesProvider")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl OpenAiResponsesProvider {
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_endpoint(api_key, "https://api.openai.com/v1/responses")
    }

    #[must_use]
    pub fn with_endpoint(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
            api_key: api_key.into(),
        }
    }
}

impl ModelProvider for OpenAiResponsesProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    #[instrument(skip(self, request), fields(provider = self.name(), model = %request.model))]
    async fn invoke(
        &self,
        request: ModelProviderRequest,
    ) -> Result<ModelProviderResponse, ModelProviderFailure> {
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&OpenAiRequest {
                model: &request.model,
                input: &request.input,
                store: false,
            })
            .send()
            .await
            .map_err(transport_failure)?;
        let status = response.status();
        let provider_request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await.map_err(transport_failure)?;
        if !status.is_success() {
            let error = serde_json::from_slice::<OpenAiErrorEnvelope>(&body).ok();
            return Err(ModelProviderFailure {
                status_code: Some(status.as_u16()),
                code: error.as_ref().and_then(|value| value.error.code.clone()),
                message: error
                    .map(|value| value.error.message)
                    .unwrap_or_else(|| format!("OpenAI returned HTTP {status}")),
                body,
                provider_request_id,
            });
        }
        let parsed = serde_json::from_slice::<OpenAiResponse>(&body).map_err(|error| {
            ModelProviderFailure {
                status_code: Some(status.as_u16()),
                code: Some("invalid_response".to_owned()),
                message: error.to_string(),
                body: body.clone(),
                provider_request_id: provider_request_id.clone(),
            }
        })?;
        Ok(ModelProviderResponse {
            body,
            usage: ModelTokenUsage {
                input_tokens: parsed.usage.input_tokens,
                output_tokens: parsed.usage.output_tokens,
            },
            provider_request_id,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PgModelInvocationService<P, A> {
    pool: PgPool,
    provider: P,
    artifacts: A,
}

impl<P, A> PgModelInvocationService<P, A> {
    #[must_use]
    pub fn new(pool: PgPool, provider: P, artifacts: A) -> Self {
        Self {
            pool,
            provider,
            artifacts,
        }
    }
}

impl<P, A> ModelInvocationService for PgModelInvocationService<P, A>
where
    P: ModelProvider,
    A: ArtifactStore,
{
    #[instrument(skip(self, invocation), fields(invocation_id = %invocation.id, run_id = %invocation.run_id, provider = self.provider.name(), model = %invocation.model))]
    async fn invoke(
        &self,
        invocation: NewModelInvocation,
    ) -> Result<ModelInvocation, ModelInvocationError> {
        sqlx::query(
            "INSERT INTO model_invocations (id, run_id, context_snapshot_id, provider, model, request_artifact_id, status, created_at) VALUES ($1, $2, $3, $4, $5, $6, 'running', to_timestamp($7::double precision / 1000))",
        )
        .bind(invocation.id.as_uuid())
        .bind(invocation.run_id.as_uuid())
        .bind(invocation.context_snapshot_id.as_uuid())
        .bind(self.provider.name())
        .bind(&invocation.model)
        .bind(invocation.request_artifact_id.as_uuid())
        .bind(invocation.created_at.unix_milliseconds())
        .execute(&self.pool)
        .await
        .map_err(storage)?;

        let started = Instant::now();
        let input = self.load_request(invocation.request_artifact_id).await?;
        let outcome = self
            .provider
            .invoke(ModelProviderRequest {
                model: invocation.model.clone(),
                input,
            })
            .await;
        let latency_milliseconds = started.elapsed().as_millis() as u64;
        let completed_at = now();

        match outcome {
            Ok(response) => {
                let response_artifact_id = ArtifactId::new();
                self.store_response(
                    invocation.run_id,
                    response_artifact_id,
                    response.body,
                    completed_at,
                )
                .await?;
                sqlx::query(
                    "UPDATE model_invocations SET response_artifact_id = $2, status = 'succeeded', input_tokens = $3, output_tokens = $4, latency_milliseconds = $5, provider_request_id = $6, completed_at = to_timestamp($7::double precision / 1000) WHERE id = $1",
                )
                .bind(invocation.id.as_uuid())
                .bind(response_artifact_id.as_uuid())
                .bind(response.usage.input_tokens as i64)
                .bind(response.usage.output_tokens as i64)
                .bind(latency_milliseconds as i64)
                .bind(&response.provider_request_id)
                .bind(completed_at.unix_milliseconds())
                .execute(&self.pool)
                .await
                .map_err(storage)?;
                info!(latency_milliseconds, "model invocation succeeded");
                Ok(ModelInvocation {
                    id: invocation.id,
                    run_id: invocation.run_id,
                    context_snapshot_id: invocation.context_snapshot_id,
                    provider: self.provider.name().to_owned(),
                    model: invocation.model,
                    request_artifact_id: invocation.request_artifact_id,
                    response_artifact_id: Some(response_artifact_id),
                    status: ModelInvocationStatus::Succeeded,
                    usage: Some(response.usage),
                    latency_milliseconds: Some(latency_milliseconds),
                    provider_request_id: response.provider_request_id,
                    error_code: None,
                    error_message: None,
                    created_at: invocation.created_at,
                    completed_at: Some(completed_at),
                })
            }
            Err(failure) => {
                let response_artifact_id = ArtifactId::new();
                self.store_response(
                    invocation.run_id,
                    response_artifact_id,
                    failure.body.clone(),
                    completed_at,
                )
                .await?;
                sqlx::query(
                    "UPDATE model_invocations SET response_artifact_id = $2, status = 'failed', latency_milliseconds = $3, provider_request_id = $4, error_code = $5, error_message = $6, completed_at = to_timestamp($7::double precision / 1000) WHERE id = $1",
                )
                .bind(invocation.id.as_uuid())
                .bind(response_artifact_id.as_uuid())
                .bind(latency_milliseconds as i64)
                .bind(&failure.provider_request_id)
                .bind(&failure.code)
                .bind(&failure.message)
                .bind(completed_at.unix_milliseconds())
                .execute(&self.pool)
                .await
                .map_err(storage)?;
                warn!(
                    latency_milliseconds,
                    status_code = failure.status_code,
                    error_code = failure.code,
                    "model invocation failed"
                );
                Err(ModelInvocationError::Provider {
                    invocation_id: invocation.id,
                    failure,
                })
            }
        }
    }
}

impl<P, A> PgModelInvocationService<P, A>
where
    A: ArtifactStore,
{
    async fn load_request(
        &self,
        request_artifact_id: ArtifactId,
    ) -> Result<serde_json::Value, ModelInvocationError> {
        let mut stream = self
            .artifacts
            .read(request_artifact_id)
            .await
            .map_err(storage)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.map_err(storage)?);
        }
        serde_json::from_slice(&bytes).map_err(storage)
    }

    async fn store_response(
        &self,
        run_id: mc_domain::RunId,
        id: ArtifactId,
        body: Bytes,
        created_at: Timestamp,
    ) -> Result<(), ModelInvocationError> {
        self.artifacts
            .put(
                NewArtifact {
                    id,
                    run_id,
                    kind: ArtifactKind::ModelPayload,
                    media_type: "application/json".to_owned(),
                    created_at,
                },
                Box::pin(stream::once(async { Ok(body) })),
            )
            .await
            .map_err(storage)?;
        Ok(())
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    input: &'a serde_json::Value,
    store: bool,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiError,
}

#[derive(Deserialize)]
struct OpenAiError {
    code: Option<String>,
    message: String,
}

fn transport_failure(error: reqwest::Error) -> ModelProviderFailure {
    let message = error.to_string();
    ModelProviderFailure {
        status_code: error.status().map(|status| status.as_u16()),
        code: Some("transport_error".to_owned()),
        message: message.clone(),
        body: Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "error": {
                    "code": "transport_error",
                    "message": message
                }
            }))
            .expect("transport failure JSON is serializable"),
        ),
        provider_request_id: None,
    }
}

fn now() -> Timestamp {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    Timestamp::from_unix_milliseconds(milliseconds)
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> ModelInvocationError {
    ModelInvocationError::Storage(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::{OpenAiErrorEnvelope, OpenAiRequest, OpenAiResponse, OpenAiResponsesProvider};

    #[test]
    fn openai_request_uses_responses_shape_without_remote_storage() {
        let input = serde_json::json!([{"role": "user", "content": "hello"}]);
        let encoded = serde_json::to_value(OpenAiRequest {
            model: "test-model-v1",
            input: &input,
            store: false,
        })
        .unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "model": "test-model-v1",
                "input": input,
                "store": false
            })
        );
    }

    #[test]
    fn openai_response_contract_normalizes_usage_and_errors() {
        let response: OpenAiResponse = serde_json::from_value(serde_json::json!({
            "usage": {"input_tokens": 13, "output_tokens": 8}
        }))
        .unwrap();
        assert_eq!(response.usage.input_tokens, 13);
        assert_eq!(response.usage.output_tokens, 8);

        let error: OpenAiErrorEnvelope = serde_json::from_value(serde_json::json!({
            "error": {"code": "rate_limit_exceeded", "message": "slow down"}
        }))
        .unwrap();
        assert_eq!(error.error.code.as_deref(), Some("rate_limit_exceeded"));
        assert_eq!(error.error.message, "slow down");
    }

    #[test]
    fn provider_debug_output_redacts_credentials() {
        let provider = OpenAiResponsesProvider::new("top-secret-key");
        let debug = format!("{provider:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("top-secret-key"));
    }
}
