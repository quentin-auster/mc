use sqlx::{Executor, PgPool};
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn seed_run(pool: &PgPool) -> (Uuid, Uuid) {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();

    pool.execute(
        sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, $2)")
            .bind(repository_id)
            .bind("https://example.invalid/repository.git"),
    )
    .await
    .unwrap();
    pool.execute(
        sqlx::query(
            "INSERT INTO repository_snapshots (id, repository_id, tree_digest) VALUES ($1, $2, $3)",
        )
        .bind(snapshot_id)
        .bind(repository_id)
        .bind(DIGEST),
    )
    .await
    .unwrap();
    pool.execute(
        sqlx::query("INSERT INTO tasks (id, repository_id) VALUES ($1, $2)")
            .bind(task_id)
            .bind(repository_id),
    )
    .await
    .unwrap();
    pool.execute(
        sqlx::query("INSERT INTO task_contracts (task_id, version, objective) VALUES ($1, 1, $2)")
            .bind(task_id)
            .bind("Implement the requested behavior"),
    )
    .await
    .unwrap();
    pool.execute(
        sqlx::query(
            "INSERT INTO runs (id, repository_id, task_id, task_contract_version, snapshot_id, status) VALUES ($1, $2, $3, 1, $4, 'pending')",
        )
        .bind(run_id)
        .bind(repository_id)
        .bind(task_id)
        .bind(snapshot_id),
    )
    .await
    .unwrap();

    (run_id, snapshot_id)
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored migration tests explicitly"]
async fn schema_persists_a_reproducible_invocation_graph(pool: PgPool) {
    let (run_id, snapshot_id) = seed_run(&pool).await;
    let event_id = Uuid::new_v4();
    let artifact_id = Uuid::new_v4();
    let context_snapshot_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO run_events (id, run_id, sequence, occurred_at, kind, payload_version, payload) VALUES ($1, $2, 1, now(), 'run_started', 1, '{}'::jsonb)",
    )
    .bind(event_id)
    .bind(run_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO artifacts (id, run_id, kind, digest, media_type, byte_length, object_key) VALUES ($1, $2, 'model_payload', $3, 'application/json', 2, $4)",
    )
    .bind(artifact_id)
    .bind(run_id)
    .bind(DIGEST)
    .bind(format!("runs/{run_id}/request.json"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO context_snapshots (id, run_id, policy_version, rendered_digest, estimated_tokens) VALUES ($1, $2, 1, $3, 12)",
    )
    .bind(context_snapshot_id)
    .bind(run_id)
    .bind(DIGEST)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO context_items (id, context_snapshot_id, run_id, sequence, kind, source, inclusion_reason, estimated_tokens, rendered_digest) VALUES ($1, $2, $3, 1, 'repository_evidence', $4, 'task evidence', 12, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(context_snapshot_id)
    .bind(run_id)
    .bind(serde_json::json!({"kind": "snapshot_path", "snapshot_id": snapshot_id}))
    .bind(DIGEST)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO model_invocations (id, run_id, context_snapshot_id, provider, model, request_artifact_id) VALUES ($1, $2, $3, 'test-provider', 'test-model', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(run_id)
    .bind(context_snapshot_id)
    .bind(artifact_id)
    .execute(&pool)
    .await
    .unwrap();

    let invocation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_invocations WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(invocation_count, 1);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored migration tests explicitly"]
async fn schema_rejects_duplicate_event_sequence(pool: PgPool) {
    let (run_id, _) = seed_run(&pool).await;
    let statement = "INSERT INTO run_events (id, run_id, sequence, occurred_at, kind, payload_version, payload) VALUES ($1, $2, 1, now(), 'test', 1, '{}'::jsonb)";

    sqlx::query(statement)
        .bind(Uuid::new_v4())
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
    let error = sqlx::query(statement)
        .bind(Uuid::new_v4())
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap_err();

    assert!(error.as_database_error().is_some_and(|database_error| {
        database_error.constraint() == Some("run_events_run_id_sequence_key")
    }));
}
