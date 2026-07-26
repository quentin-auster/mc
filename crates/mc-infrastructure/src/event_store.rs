use std::time::Duration;

use async_stream::try_stream;
use mc_application::{EventStore, EventStoreError, EventStream, NewRunEvent};
use mc_domain::{EventId, EventPayload, RunEvent, RunId, Sequence, Timestamp, Version};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use tracing::{info, instrument};

#[derive(Clone, Debug)]
pub struct PgEventStore {
    pool: PgPool,
    poll_interval: Duration,
}

impl PgEventStore {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            poll_interval: Duration::from_millis(100),
        }
    }

    #[must_use]
    pub fn with_poll_interval(pool: PgPool, poll_interval: Duration) -> Self {
        Self {
            pool,
            poll_interval,
        }
    }
}

impl EventStore for PgEventStore {
    #[instrument(skip(self, event), fields(run_id = %event.run_id, event_id = %event.id, event_kind = %event.payload.kind))]
    async fn append(&self, event: NewRunEvent) -> Result<RunEvent, EventStoreError> {
        let mut transaction = self.pool.begin().await.map_err(storage)?;
        lock_run(&mut transaction, event.run_id).await?;

        if let Some(existing) = find_event(&mut transaction, event.id).await? {
            if same_event(&existing, &event) {
                transaction.commit().await.map_err(storage)?;
                info!(
                    sequence = existing.sequence.get(),
                    idempotent = true,
                    "event appended"
                );
                return Ok(existing);
            }
            return Err(EventStoreError::IdempotencyConflict { event_id: event.id });
        }

        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(sequence), 0) + 1 FROM run_events WHERE run_id = $1",
        )
        .bind(event.run_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage)?;
        sqlx::query(
            "INSERT INTO run_events (id, run_id, sequence, occurred_at, causation_id, correlation_id, kind, payload_version, payload) VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000), $5, $6, $7, $8, $9)",
        )
        .bind(event.id.as_uuid())
        .bind(event.run_id.as_uuid())
        .bind(sequence)
        .bind(event.occurred_at.unix_milliseconds())
        .bind(event.causation_id.map(EventId::as_uuid))
        .bind(&event.correlation_id)
        .bind(&event.payload.kind)
        .bind(i64::from(event.payload.version.get()))
        .bind(&event.payload.data)
        .execute(&mut *transaction)
        .await
        .map_err(storage)?;
        transaction.commit().await.map_err(storage)?;

        let appended = RunEvent {
            id: event.id,
            run_id: event.run_id,
            sequence: Sequence::new(sequence as u64).expect("database sequence is positive"),
            occurred_at: event.occurred_at,
            causation_id: event.causation_id,
            correlation_id: event.correlation_id,
            payload: event.payload,
        };
        info!(sequence, idempotent = false, "event appended");
        Ok(appended)
    }

    #[instrument(skip(self), fields(%run_id, after = after.map(Sequence::get)))]
    async fn replay(
        &self,
        run_id: RunId,
        after: Option<Sequence>,
    ) -> Result<Vec<RunEvent>, EventStoreError> {
        let run_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id = $1)")
                .bind(run_id.as_uuid())
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        if !run_exists {
            return Err(EventStoreError::RunNotFound { run_id });
        }
        let rows = sqlx::query(
            "SELECT id, run_id, sequence, floor(extract(epoch FROM occurred_at) * 1000)::bigint AS occurred_at_ms, causation_id, correlation_id, kind, payload_version, payload FROM run_events WHERE run_id = $1 AND sequence > $2 ORDER BY sequence",
        )
        .bind(run_id.as_uuid())
        .bind(after.map_or(0, |sequence| sequence.get() as i64))
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?;
        let events: Vec<RunEvent> = rows
            .into_iter()
            .map(row_to_event)
            .collect::<Result<_, _>>()?;
        info!(event_count = events.len(), "event stream replayed");
        Ok(events)
    }

    fn subscribe(&self, run_id: RunId, after: Option<Sequence>) -> EventStream<'_> {
        let poll_interval = self.poll_interval;
        Box::pin(try_stream! {
            let mut cursor = after;
            info!(%run_id, after = cursor.map(Sequence::get), "event subscription started");
            loop {
                let events = self.replay(run_id, cursor).await?;
                for event in events {
                    cursor = Some(event.sequence);
                    yield event;
                }
                tokio::time::sleep(poll_interval).await;
            }
        })
    }
}

async fn lock_run(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: RunId,
) -> Result<(), EventStoreError> {
    let exists = sqlx::query_scalar::<_, bool>("SELECT true FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run_id.as_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(storage)?
        .unwrap_or(false);
    if exists {
        Ok(())
    } else {
        Err(EventStoreError::RunNotFound { run_id })
    }
}

async fn find_event(
    transaction: &mut Transaction<'_, Postgres>,
    event_id: EventId,
) -> Result<Option<RunEvent>, EventStoreError> {
    sqlx::query(
        "SELECT id, run_id, sequence, floor(extract(epoch FROM occurred_at) * 1000)::bigint AS occurred_at_ms, causation_id, correlation_id, kind, payload_version, payload FROM run_events WHERE id = $1",
    )
    .bind(event_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage)?
    .map(row_to_event)
    .transpose()
}

fn row_to_event(row: PgRow) -> Result<RunEvent, EventStoreError> {
    let sequence: i64 = row.try_get("sequence").map_err(storage)?;
    let payload_version: i32 = row.try_get("payload_version").map_err(storage)?;
    Ok(RunEvent {
        id: EventId::from_uuid(row.try_get("id").map_err(storage)?),
        run_id: RunId::from_uuid(row.try_get("run_id").map_err(storage)?),
        sequence: Sequence::new(sequence as u64).map_err(storage)?,
        occurred_at: Timestamp::from_unix_milliseconds(
            row.try_get("occurred_at_ms").map_err(storage)?,
        ),
        causation_id: row
            .try_get::<Option<_>, _>("causation_id")
            .map_err(storage)?
            .map(EventId::from_uuid),
        correlation_id: row.try_get("correlation_id").map_err(storage)?,
        payload: EventPayload {
            kind: row.try_get("kind").map_err(storage)?,
            version: Version::new(payload_version as u32).map_err(storage)?,
            data: row.try_get("payload").map_err(storage)?,
        },
    })
}

fn same_event(existing: &RunEvent, candidate: &NewRunEvent) -> bool {
    existing.id == candidate.id
        && existing.run_id == candidate.run_id
        && existing.occurred_at == candidate.occurred_at
        && existing.causation_id == candidate.causation_id
        && existing.correlation_id == candidate.correlation_id
        && existing.payload == candidate.payload
}

fn storage(error: impl std::error::Error + Send + Sync + 'static) -> EventStoreError {
    EventStoreError::Storage(Box::new(error))
}
