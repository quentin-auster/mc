# 0005: Make repository snapshots immutable

- Status: Accepted
- Date: 2026-07-22

## Context

Repository contents change during and between tasks. A branch name or working directory path does
not identify the exact evidence observed by an agent, so it cannot support reproducible retrieval
or later claim validation.

## Decision

Represent each observed repository state as an immutable snapshot with a stable identifier,
repository identity, source revision when available, tree/content digest, creation provenance, and
parent relationship. Changes create a new snapshot. Context items and evidence refer to snapshot
identities rather than mutable paths alone.

## Consequences

- Retrieval and model inputs can be reproduced against exact repository state.
- Storage may deduplicate content but must not alter the logical snapshot.
- Mutable worktrees are execution surfaces, not authoritative snapshot identity.
- Garbage collection must retain snapshots referenced by runs, claims, or context records.
