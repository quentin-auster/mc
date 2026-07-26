use bytes::Bytes;
use futures_util::{StreamExt, stream};
use mc_application::{
    ArtifactStore, ModelInvocationError, ModelInvocationService, ModelProvider,
    ModelProviderFailure, ModelProviderRequest, ModelProviderResponse, NewArtifact,
    NewModelInvocation,
};
use mc_domain::{
    ArtifactId, ArtifactKind, ContextSnapshotId, InvocationId, ModelInvocationStatus,
    ModelTokenUsage, RunId, Timestamp,
};
use mc_infrastructure::{PgModelInvocationService, PgObjectArtifactStore, build_s3_store};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CREATED_AT: Timestamp = Timestamp::from_unix_milliseconds(1_750_000_000_000);

#[derive(Clone)]
struct StubProvider {
    outcome: Result<ModelProviderResponse, ModelProviderFailure>,
}

impl ModelProvider for StubProvider {
    fn name(&self) -> &'static str {
        "stub"
    }

    async fn invoke(
        &self,
        _request: ModelProviderRequest,
    ) -> Result<ModelProviderResponse, ModelProviderFailure> {
        self.outcome.clone()
    }
}

async fn seed(pool: &PgPool) -> (RunId, ContextSnapshotId) {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let context_snapshot_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'provider-test')")
        .bind(repository_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO repository_snapshots (id, repository_id, tree_digest) VALUES ($1, $2, $3)",
    )
    .bind(snapshot_id)
    .bind(repository_id)
    .bind(DIGEST)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tasks (id, repository_id) VALUES ($1, $2)")
        .bind(task_id)
        .bind(repository_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_contracts (task_id, version, objective) VALUES ($1, 1, 'test')")
        .bind(task_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, repository_id, task_id, task_contract_version, snapshot_id, status) VALUES ($1, $2, $3, 1, $4, 'pending')")
        .bind(run_id)
        .bind(repository_id)
        .bind(task_id)
        .bind(snapshot_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO context_snapshots (id, run_id, policy_version, rendered_digest, estimated_tokens) VALUES ($1, $2, 1, $3, 4)")
        .bind(context_snapshot_id)
        .bind(run_id)
        .bind(DIGEST)
        .execute(pool)
        .await
        .unwrap();
    (
        RunId::from_uuid(run_id),
        ContextSnapshotId::from_uuid(context_snapshot_id),
    )
}

async fn request_artifact(store: &PgObjectArtifactStore, run_id: RunId) -> ArtifactId {
    let id = ArtifactId::new();
    store
        .put(
            NewArtifact {
                id,
                run_id,
                kind: ArtifactKind::ModelPayload,
                media_type: "application/json".to_owned(),
                created_at: CREATED_AT,
            },
            Box::pin(stream::once(async {
                Ok(Bytes::from_static(br#"{"input":"hello"}"#))
            })),
        )
        .await
        .unwrap();
    id
}

fn invocation(
    run_id: RunId,
    context_snapshot_id: ContextSnapshotId,
    request_artifact_id: ArtifactId,
) -> NewModelInvocation {
    NewModelInvocation {
        id: InvocationId::new(),
        run_id,
        context_snapshot_id,
        model: "test-model-v1".to_owned(),
        request_artifact_id,
        created_at: CREATED_AT,
    }
}

async fn artifact_bytes(store: &PgObjectArtifactStore, id: ArtifactId) -> Vec<u8> {
    store
        .read(id)
        .await
        .unwrap()
        .map(|chunk| chunk.unwrap())
        .collect::<Vec<_>>()
        .await
        .concat()
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL and MinIO; CI runs ignored model-provider tests explicitly"]
async fn persists_success_and_failure_evidence(pool: PgPool) {
    let (run_id, context_snapshot_id) = seed(&pool).await;
    let objects = build_s3_store(
        "http://localhost:59000",
        "mc-artifacts",
        "mcdev",
        "mc_dev_only_password",
    )
    .unwrap();
    let artifacts = PgObjectArtifactStore::new(pool.clone(), objects);

    let success_service = PgModelInvocationService::new(
        pool.clone(),
        StubProvider {
            outcome: Ok(ModelProviderResponse {
                body: Bytes::from_static(br#"{"id":"resp_1","output":[]}"#),
                usage: ModelTokenUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                },
                provider_request_id: Some("request-success".to_owned()),
            }),
        },
        artifacts.clone(),
    );
    let completed = success_service
        .invoke(invocation(
            run_id,
            context_snapshot_id,
            request_artifact(&artifacts, run_id).await,
        ))
        .await
        .unwrap();
    assert_eq!(completed.status, ModelInvocationStatus::Succeeded);
    assert_eq!(
        completed.usage,
        Some(ModelTokenUsage {
            input_tokens: 11,
            output_tokens: 7
        })
    );
    assert_eq!(
        artifact_bytes(&artifacts, completed.response_artifact_id.unwrap()).await,
        br#"{"id":"resp_1","output":[]}"#
    );
    let success_row = sqlx::query(
        "SELECT provider, model, status, input_tokens, output_tokens, latency_milliseconds, provider_request_id FROM model_invocations WHERE id = $1",
    )
    .bind(completed.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(success_row.get::<String, _>("provider"), "stub");
    assert_eq!(success_row.get::<String, _>("model"), "test-model-v1");
    assert_eq!(success_row.get::<String, _>("status"), "succeeded");
    assert_eq!(success_row.get::<i64, _>("input_tokens"), 11);
    assert_eq!(success_row.get::<i64, _>("output_tokens"), 7);
    assert!(success_row.get::<i64, _>("latency_milliseconds") >= 0);
    assert_eq!(
        success_row
            .get::<Option<String>, _>("provider_request_id")
            .as_deref(),
        Some("request-success")
    );

    let failure_service = PgModelInvocationService::new(
        pool.clone(),
        StubProvider {
            outcome: Err(ModelProviderFailure {
                status_code: Some(429),
                code: Some("rate_limit_exceeded".to_owned()),
                message: "slow down".to_owned(),
                body: Bytes::from_static(
                    br#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#,
                ),
                provider_request_id: Some("request-failure".to_owned()),
            }),
        },
        artifacts.clone(),
    );
    let failed_invocation = invocation(
        run_id,
        context_snapshot_id,
        request_artifact(&artifacts, run_id).await,
    );
    let failed_id = failed_invocation.id;
    let error = failure_service.invoke(failed_invocation).await.unwrap_err();
    assert!(matches!(
        error,
        ModelInvocationError::Provider {
            invocation_id,
            ..
        } if invocation_id == failed_id
    ));

    let row = sqlx::query(
        "SELECT status, input_tokens, output_tokens, latency_milliseconds, provider_request_id, error_code, error_message, response_artifact_id, completed_at IS NOT NULL AS completed FROM model_invocations WHERE id = $1",
    )
    .bind(failed_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(row.get::<Option<i64>, _>("input_tokens"), None);
    assert_eq!(row.get::<Option<i64>, _>("output_tokens"), None);
    assert!(row.get::<i64, _>("latency_milliseconds") >= 0);
    assert_eq!(
        row.get::<Option<String>, _>("provider_request_id")
            .as_deref(),
        Some("request-failure")
    );
    assert_eq!(
        row.get::<Option<String>, _>("error_code").as_deref(),
        Some("rate_limit_exceeded")
    );
    assert_eq!(
        row.get::<Option<String>, _>("error_message").as_deref(),
        Some("slow down")
    );
    assert!(row.get::<bool, _>("completed"));
    let response_artifact_id = ArtifactId::from_uuid(row.get::<Uuid, _>("response_artifact_id"));
    assert_eq!(
        artifact_bytes(&artifacts, response_artifact_id).await,
        br#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#
    );
}
