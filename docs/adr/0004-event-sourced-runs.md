# 0004: Model runs as event streams

- Status: Accepted
- Date: 2026-07-22

## Context

An agent run changes through planning, model invocations, tool calls, approvals, failures, retries,
and completion. Storing only the latest status hides how that status was reached and makes recovery
and debugging ambiguous.

## Decision

Record each run transition as an ordered, append-only, versioned event. Build current run state as
a deterministic projection of that stream. Events carry stable identifiers, sequence numbers,
timestamps, typed payload versions, causation, and correlation metadata. Corrections append new
events; they do not mutate history.

## Consequences

- Run histories are auditable and projections can be rebuilt.
- Event ordering and idempotent append behavior are explicit invariants.
- Payload evolution requires versioning and upcasting or multi-version projection logic.
- Sensitive fields must be redacted or referenced as protected artifacts before event persistence.
