# Domain model

`mc-domain` contains provider-neutral records and invariants. It has no database, object-storage,
HTTP, model-provider, or runtime dependencies.

## Core records

- `Repository` identifies a registered source; `RepositorySnapshot` identifies one immutable tree.
- `Task` owns a versioned `TaskContract`; `Run` executes that contract against one snapshot.
- `RunEvent` is an ordered event with a versioned JSON payload, causation, and correlation metadata.
- `ToolCall` links typed execution state to argument and result artifacts.
- `Artifact` records an object key plus content digest, media type, size, kind, and provenance.
- `ModelInvocation` links a provider-neutral provider/model label to request, response, context,
  terminal status, token use, latency, provider request ID, and structured failure evidence.
- `ContextSnapshot` and ordered `ContextItem` records preserve policy version, source evidence,
  rendered digest, token estimate, inclusion reason, and compaction lineage.
- `Claim` is repository- or snapshot-scoped and is supported by explicit `Evidence` records.

Every aggregate identity uses a distinct UUID-backed type. Versions and event/context sequences are
non-zero. Content integrity uses validated lowercase SHA-256 digests. Timestamps are Unix
milliseconds in UTC; callers are responsible for obtaining them from a trustworthy clock.

## Trust boundary

These records describe identity and provenance, but strings originating from repositories, tools,
users, or model providers remain untrusted at output boundaries. Adapters must parameterize SQL,
escape rendered HTML, and avoid interpolating these values into shell commands.

Secrets and unrestricted payload bodies do not belong in domain events or context metadata. Store
protected bodies as access-controlled artifacts and retain only the authorized reference, digest,
and provenance in canonical records. Repository locators and object keys are internal metadata and
must not be exposed without authorization checks.
