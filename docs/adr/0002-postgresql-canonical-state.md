# 0002: Keep canonical state in PostgreSQL

- Status: Accepted
- Date: 2026-07-22

## Context

Agent runs must survive process restarts and be inspectable without relying on a model transcript.
Repositories, tasks, runs, events, context provenance, and durable claims need transactional
relationships and enforceable integrity constraints.

## Decision

Use PostgreSQL as the canonical store for structured platform state. Schema changes are versioned
migrations. Services must not create or repair schema implicitly at runtime. Caches, search
indexes, and projections are derived state and must be rebuildable from canonical records and
evidence.

## Consequences

- Transactions and database constraints protect cross-record invariants.
- Local development and production require PostgreSQL.
- Derived stores may improve latency but cannot become the sole source of truth.
- Backups and recovery procedures center on PostgreSQL plus referenced artifact objects.
