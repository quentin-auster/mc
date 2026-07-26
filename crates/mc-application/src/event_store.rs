use std::{error::Error, pin::Pin};

use futures_core::Stream;
use mc_domain::{EventId, EventPayload, RunEvent, RunId, Sequence, Timestamp};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRunEvent {
    pub id: EventId,
    pub run_id: RunId,
    pub occurred_at: Timestamp,
    pub causation_id: Option<EventId>,
    pub correlation_id: Option<String>,
    pub payload: EventPayload,
}

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("run {run_id} does not exist")]
    RunNotFound { run_id: RunId },
    #[error("event {event_id} was already appended with different content")]
    IdempotencyConflict { event_id: EventId },
    #[error("event store failed: {0}")]
    Storage(#[source] Box<dyn Error + Send + Sync>),
}

pub type EventStream<'a> =
    Pin<Box<dyn Stream<Item = Result<RunEvent, EventStoreError>> + Send + 'a>>;

pub trait EventStore: Send + Sync {
    fn append(
        &self,
        event: NewRunEvent,
    ) -> impl Future<Output = Result<RunEvent, EventStoreError>> + Send;

    fn replay(
        &self,
        run_id: RunId,
        after: Option<Sequence>,
    ) -> impl Future<Output = Result<Vec<RunEvent>, EventStoreError>> + Send;

    fn subscribe(&self, run_id: RunId, after: Option<Sequence>) -> EventStream<'_>;
}
