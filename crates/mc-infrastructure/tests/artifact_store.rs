use futures_util::{StreamExt, stream};
use mc_application::{ArtifactStore, ArtifactStoreError, NewArtifact};
use mc_domain::{ArtifactId, ArtifactKind, RunId, Timestamp};
use mc_infrastructure::{PgObjectArtifactStore, build_s3_store};
use sqlx::PgPool;
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn seed_run(pool: &PgPool) -> RunId {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'artifact-test')")
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
        .bind(run_id).bind(repository_id).bind(task_id).bind(snapshot_id).execute(pool).await.unwrap();
    RunId::from_uuid(run_id)
}

fn upload(run_id: RunId, id: ArtifactId) -> NewArtifact {
    NewArtifact {
        id,
        run_id,
        kind: ArtifactKind::Log,
        media_type: "text/plain".to_owned(),
        created_at: Timestamp::from_unix_milliseconds(1_750_000_000_000),
    }
}

fn body() -> mc_application::ArtifactBody {
    Box::pin(stream::iter([
        Ok("first line\n".into()),
        Ok("second line\n".into()),
    ]))
}

fn empty_body() -> mc_application::ArtifactBody {
    Box::pin(stream::empty())
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL and MinIO; CI runs ignored artifact-store tests explicitly"]
async fn stores_deduplicated_streams_and_reads_ranges(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let objects = build_s3_store(
        "http://localhost:59000",
        "mc-artifacts",
        "mcdev",
        "mc_dev_only_password",
    )
    .unwrap();
    let store = PgObjectArtifactStore::new(pool.clone(), objects);
    let first_id = ArtifactId::new();
    let second_id = ArtifactId::new();

    let first = store.put(upload(run_id, first_id), body()).await.unwrap();
    let second = store.put(upload(run_id, second_id), body()).await.unwrap();
    assert_eq!(first.digest, second.digest);
    assert_eq!(first.object_key, second.object_key);

    let metadata_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifacts WHERE object_key = $1")
            .bind(&first.object_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(metadata_count, 2);

    let bytes = store
        .read(first_id)
        .await
        .unwrap()
        .map(|chunk| chunk.unwrap())
        .collect::<Vec<_>>()
        .await
        .concat();
    assert_eq!(bytes, b"first line\nsecond line\n");
    assert_eq!(store.read_range(first_id, 11..17).await.unwrap(), "second");

    assert!(matches!(
        store.read_range(first_id, 5..50).await.unwrap_err(),
        ArtifactStoreError::InvalidRange { .. }
    ));
    let missing = store.read(ArtifactId::new()).await;
    assert!(matches!(missing, Err(ArtifactStoreError::NotFound { .. })));

    let empty_id = ArtifactId::new();
    let empty = store
        .put(upload(run_id, empty_id), empty_body())
        .await
        .unwrap();
    assert_eq!(empty.byte_length, 0);
    assert!(
        store
            .read(empty_id)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await
            .is_empty()
    );
}
