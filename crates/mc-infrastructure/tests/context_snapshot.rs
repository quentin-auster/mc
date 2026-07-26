use bytes::Bytes;
use mc_application::{
    ContextSnapshotError, ContextSnapshotStore, NewContextSnapshot, RenderedContextItem,
};
use mc_domain::{
    ContextItemId, ContextItemKind, ContextSnapshotId, EvidenceSource, RunId, Sha256Digest,
    SnapshotId, Timestamp, Version,
};
use mc_infrastructure::{PgContextSnapshotStore, PgObjectArtifactStore, build_s3_store};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CREATED_AT: Timestamp = Timestamp::from_unix_milliseconds(1_750_000_000_000);

async fn seed(pool: &PgPool) -> (RunId, SnapshotId) {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'context-test')")
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
    (RunId::from_uuid(run_id), SnapshotId::from_uuid(snapshot_id))
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL and MinIO; CI runs ignored context-snapshot tests explicitly"]
async fn persists_and_reconstructs_exact_ordered_context(pool: PgPool) {
    let (run_id, repository_snapshot_id) = seed(&pool).await;
    let objects = build_s3_store(
        "http://localhost:59000",
        "mc-artifacts",
        "mcdev",
        "mc_dev_only_password",
    )
    .unwrap();
    let artifacts = PgObjectArtifactStore::new(pool.clone(), objects);
    let store = PgContextSnapshotStore::new(pool.clone(), artifacts);
    let snapshot_id = ContextSnapshotId::new();
    let instruction_id = ContextItemId::new();
    let evidence_id = ContextItemId::new();
    let snapshot = NewContextSnapshot {
        id: snapshot_id,
        run_id,
        policy_version: Version::new(3).unwrap(),
        items: vec![
            RenderedContextItem {
                id: instruction_id,
                kind: ContextItemKind::Instruction,
                source: EvidenceSource::Event {
                    event_id: mc_domain::EventId::new(),
                },
                inclusion_reason: "required system instruction".to_owned(),
                estimated_tokens: 4,
                rendered: Bytes::from_static(b"system\n"),
                compacted_from: Vec::new(),
            },
            RenderedContextItem {
                id: evidence_id,
                kind: ContextItemKind::RepositoryEvidence,
                source: EvidenceSource::SnapshotPath {
                    snapshot_id: repository_snapshot_id,
                    path: "src/lib.rs".to_owned(),
                    content_digest: Sha256Digest::new(DIGEST).unwrap(),
                },
                inclusion_reason: "retrieved for the active task".to_owned(),
                estimated_tokens: 7,
                rendered: Bytes::from_static(b"user evidence"),
                compacted_from: vec![instruction_id],
            },
        ],
        created_at: CREATED_AT,
    };

    let persisted = store.persist(snapshot).await.unwrap();
    let expected = b"system\nuser evidence";
    assert_eq!(
        store.reconstruct(snapshot_id).await.unwrap(),
        expected.as_slice()
    );
    assert_eq!(persisted.policy_version.get(), 3);
    assert_eq!(persisted.estimated_tokens, 11);
    assert_eq!(
        persisted.rendered_digest.as_str(),
        format!("{:x}", Sha256::digest(expected))
    );

    let rows = sqlx::query(
        "SELECT sequence, kind, source, inclusion_reason, estimated_tokens, rendered_digest, rendered_artifact_id, compacted_from FROM context_items WHERE context_snapshot_id = $1 ORDER BY sequence",
    )
    .bind(snapshot_id.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<i64, _>("sequence"), 1);
    assert_eq!(rows[1].get::<i64, _>("sequence"), 2);
    assert_eq!(rows[1].get::<String, _>("kind"), "repository_evidence");
    assert_eq!(rows[1].get::<i64, _>("estimated_tokens"), 7);
    assert_eq!(
        rows[1].get::<String, _>("inclusion_reason"),
        "retrieved for the active task"
    );
    assert_eq!(
        rows[1].get::<serde_json::Value, _>("source")["path"],
        "src/lib.rs"
    );
    assert!(rows[1].get::<Uuid, _>("rendered_artifact_id") != Uuid::nil());
    assert_eq!(
        rows[1].get::<Vec<Uuid>, _>("compacted_from"),
        vec![instruction_id.as_uuid()]
    );
    assert_eq!(
        rows[1].get::<String, _>("rendered_digest"),
        format!("{:x}", Sha256::digest(b"user evidence"))
    );

    let missing = store.reconstruct(ContextSnapshotId::new()).await;
    assert!(matches!(
        missing,
        Err(ContextSnapshotError::NotFound { .. })
    ));

    let invalid_id = ContextSnapshotId::new();
    let invalid = store
        .persist(NewContextSnapshot {
            id: invalid_id,
            run_id,
            policy_version: Version::new(3).unwrap(),
            items: vec![RenderedContextItem {
                id: ContextItemId::new(),
                kind: ContextItemKind::Conversation,
                source: EvidenceSource::Event {
                    event_id: mc_domain::EventId::new(),
                },
                inclusion_reason: " ".to_owned(),
                estimated_tokens: 1,
                rendered: Bytes::from_static(b"unsafe"),
                compacted_from: Vec::new(),
            }],
            created_at: CREATED_AT,
        })
        .await;
    assert!(matches!(
        invalid,
        Err(ContextSnapshotError::EmptyInclusionReason { .. })
    ));
    let invalid_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM context_snapshots WHERE id = $1")
            .bind(invalid_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(invalid_count, 0);
}
