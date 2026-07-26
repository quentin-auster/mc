use std::{sync::Arc, time::Duration};

use futures_util::{StreamExt, future::join_all};
use mc_application::{EventStore, EventStoreError, NewRunEvent};
use mc_domain::{EventId, EventPayload, RunId, Timestamp, Version};
use mc_infrastructure::PgEventStore;
use sqlx::PgPool;
use uuid::Uuid;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn seed_run(pool: &PgPool) -> RunId {
    let repository_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();

    sqlx::query("INSERT INTO repositories (id, locator) VALUES ($1, 'test-repository')")
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
    sqlx::query(
        "INSERT INTO runs (id, repository_id, task_id, task_contract_version, snapshot_id, status) VALUES ($1, $2, $3, 1, $4, 'pending')",
    )
    .bind(run_id)
    .bind(repository_id)
    .bind(task_id)
    .bind(snapshot_id)
    .execute(pool)
    .await
    .unwrap();

    RunId::from_uuid(run_id)
}

fn event(run_id: RunId, id: EventId, value: usize) -> NewRunEvent {
    NewRunEvent {
        id,
        run_id,
        occurred_at: Timestamp::from_unix_milliseconds(1_750_000_000_000 + value as i64),
        causation_id: None,
        correlation_id: Some("test".to_owned()),
        payload: EventPayload::new(
            "test_event",
            Version::new(1).unwrap(),
            serde_json::json!({"value": value}),
        )
        .unwrap(),
    }
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored event-store tests explicitly"]
async fn concurrent_appends_receive_monotonic_sequences(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let store = Arc::new(PgEventStore::new(pool));
    let appends = (0..12).map(|value| {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            store
                .append(event(run_id, EventId::new(), value))
                .await
                .unwrap()
        })
    });

    let mut sequences = join_all(appends)
        .await
        .into_iter()
        .map(|result| result.unwrap().sequence.get())
        .collect::<Vec<_>>();
    sequences.sort_unstable();

    assert_eq!(sequences, (1..=12).collect::<Vec<_>>());
    assert_eq!(store.replay(run_id, None).await.unwrap().len(), 12);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored event-store tests explicitly"]
async fn append_is_idempotent_but_rejects_changed_content(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let store = PgEventStore::new(pool);
    let event_id = EventId::new();
    let original = event(run_id, event_id, 1);

    let first = store.append(original.clone()).await.unwrap();
    let retried = store.append(original).await.unwrap();
    assert_eq!(first, retried);

    let error = store.append(event(run_id, event_id, 2)).await.unwrap_err();
    assert!(matches!(
        error,
        EventStoreError::IdempotencyConflict { event_id: id } if id == event_id
    ));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored event-store tests explicitly"]
async fn subscription_delivers_new_events_in_order(pool: PgPool) {
    let run_id = seed_run(&pool).await;
    let store = PgEventStore::with_poll_interval(pool, Duration::from_millis(10));
    let mut subscription = store.subscribe(run_id, None);

    store
        .append(event(run_id, EventId::new(), 1))
        .await
        .unwrap();
    store
        .append(event(run_id, EventId::new(), 2))
        .await
        .unwrap();

    let first = tokio::time::timeout(Duration::from_secs(1), subscription.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(1), subscription.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!((first.sequence.get(), second.sequence.get()), (1, 2));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "requires PostgreSQL; CI runs ignored event-store tests explicitly"]
async fn append_rejects_an_unknown_run(pool: PgPool) {
    let store = PgEventStore::new(pool);
    let run_id = RunId::new();

    let error = store
        .append(event(run_id, EventId::new(), 1))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        EventStoreError::RunNotFound { run_id: id } if id == run_id
    ));

    let replay_error = store.replay(run_id, None).await.unwrap_err();
    assert!(matches!(
        replay_error,
        EventStoreError::RunNotFound { run_id: id } if id == run_id
    ));
}
