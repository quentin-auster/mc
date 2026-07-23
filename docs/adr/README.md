# Architecture decision records

ADRs capture decisions that constrain MC's architecture. Accepted ADRs remain in place; later
decisions supersede them with a new ADR rather than rewriting history.

- [0001: Use Rust for core services](0001-use-rust-for-core-services.md)
- [0002: Keep canonical state in PostgreSQL](0002-postgresql-canonical-state.md)
- [0003: Store large artifacts in object storage](0003-object-storage-artifacts.md)
- [0004: Model runs as event streams](0004-event-sourced-runs.md)
- [0005: Make repository snapshots immutable](0005-immutable-repository-snapshots.md)
- [0006: Persist context for every model call](0006-per-call-context-snapshots.md)
- [0007: Start with a container sandbox](0007-container-sandbox.md)
