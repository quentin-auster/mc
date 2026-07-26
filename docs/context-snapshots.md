# Context snapshots

Before a model invocation, the application persists a versioned context snapshot through the
provider-neutral `ContextSnapshotStore` port. Items carry their order, kind, evidence source,
selection reason, estimated token count, rendered bytes, rendered digest, and compaction lineage.

Rendered item bytes are stored as access-controlled, content-addressed artifacts. A separate
snapshot artifact contains the exact byte concatenation supplied by the ordered items, so replay
reads authoritative bytes rather than invoking a renderer that may have changed. The snapshot
digest and token total are derived during persistence, and the policy version is externally
meaningful.

Callers must remove or redact secrets before constructing `RenderedContextItem`; persistence is an
evidence boundary, not a redaction engine. Database foreign keys require new item and snapshot
artifacts to belong to the same run. Artifact writes happen before the metadata transaction, so a
failed metadata transaction can leave unreferenced content-addressed objects for lifecycle cleanup.
