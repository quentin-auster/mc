# 0007: Start with a container sandbox

- Status: Accepted
- Date: 2026-07-22

## Context

Repository tools execute code and commands influenced by users, repositories, and models. Process
isolation alone does not provide a sufficient filesystem, network, identity, or resource boundary.

## Decision

Use an ephemeral container as the initial execution sandbox. Run as a non-root user with a
read-only base filesystem, an explicitly mounted task worktree, bounded CPU/memory/process/time
resources, no host Docker socket, dropped Linux capabilities, and network access denied by default.
Grant narrower capabilities through a versioned tool policy and record approvals and executions as
run evidence.

## Consequences

- Container availability is required for sandboxed execution.
- Image identity and sandbox policy become part of reproducibility metadata.
- Containers reduce risk but do not eliminate kernel-level threats; stronger isolation may
  supersede this ADR.
- Any filesystem or network expansion must be explicit, scoped, auditable, and revocable.
