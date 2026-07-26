# Docs

Design notes and project documentation live here.

- [Local development](local-development.md)
- [Architecture decision records](adr/README.md)
- [Domain model](domain-model.md)
- [Event store](event-store.md)
- [Run-state projection](run-state-projection.md)
- [Artifact store](artifact-store.md)
- [Context snapshots](context-snapshots.md)
- [Repository registration and snapshots](repositories.md)
- [Run worktrees](worktrees.md)
- [Container sandbox](sandbox.md)

## Workspace architecture

The Rust workspace separates core policy from external systems:

- `mc-domain` owns provider-neutral models and invariants. It has no internal dependencies.
- `mc-application` owns use cases and ports. It may depend on `mc-domain`.
- `mc-infrastructure` owns database, object-storage, model-provider, and runtime adapters. It may
  depend on the application and domain crates.
- `mc-tui` is the existing interactive client.

Dependencies must point inward: domain code must not depend on application or infrastructure
code, and application code must not depend on infrastructure adapters.

## Local checks

CI runs the same three checks contributors should run before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-targets --all-features
```

Unit tests live next to the behavior they cover. Cross-crate and adapter tests belong in each
crate's `tests/` directory. Tests should assert observable behavior and cover relevant failure
paths without depending on private implementation details.

The workspace forbids unsafe Rust. Clippy warnings are currently reported rather than promoted to
errors because the pre-existing TUI has an inherited warning backlog.
