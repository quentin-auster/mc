# Event store

`mc-application::EventStore` is the append/replay/subscribe port. `PgEventStore` implements it with
the canonical PostgreSQL event stream.

- Callers assign an event UUID. Repeating the same event returns the original append result;
  reusing that UUID with different content is an idempotency conflict.
- Appends lock the owning run and allocate the next positive sequence inside one transaction, so
  concurrent writers cannot create gaps or duplicate ordering.
- Replay returns immutable events in ascending sequence and can resume after a supplied sequence.
- Subscription polls canonical state after its cursor, allowing API and worker processes to
  observe each other's commits without process-local state.

There is intentionally no update or delete method. Corrections are new versioned events. Payloads
must already be redacted or replaced by protected artifact references before append; persisted
history is not a safe place for secrets.
