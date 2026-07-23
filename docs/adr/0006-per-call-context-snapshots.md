# 0006: Persist context for every model call

- Status: Accepted
- Date: 2026-07-22

## Context

Model behavior depends on the exact instructions, conversation, retrieved evidence, tool results,
and policy decisions supplied to each invocation. Recording only a final transcript cannot explain
omissions, ordering, truncation, or token allocation.

## Decision

Before each model invocation, persist an immutable, versioned context snapshot and its ordered
items. Record each item's source, source version or digest, inclusion reason, policy version,
estimated token cost, rendered digest, and any compaction lineage. The invocation references that
snapshot. Sensitive bodies may be stored as access-controlled artifacts, but their provenance and
digests remain explicit.

## Consequences

- Model calls are evidence-linked and context construction can be inspected or replayed.
- Context policy changes are externally meaningful and require versioning.
- Storage and retention costs increase and require explicit policy.
- Secrets must be excluded or redacted before persistence, not merely hidden during display.
