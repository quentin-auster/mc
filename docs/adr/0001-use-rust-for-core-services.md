# 0001: Use Rust for core services

- Status: Accepted
- Date: 2026-07-22

## Context

MC coordinates concurrent, long-running agent work and handles untrusted repository content. The
runtime needs predictable resource use, explicit failure handling, and strong boundaries between
domain policy and infrastructure adapters.

## Decision

Implement the core domain, application, infrastructure, API, and worker services in Rust. Keep the
domain crate low-dependency and provider-neutral. Other languages may be used for repository
fixtures, generated clients, and isolated tools when they are the appropriate implementation
language.

## Consequences

- Memory and concurrency safety are enforced without a garbage-collected runtime.
- Shared domain contracts can be compiled into both API and worker processes.
- Contributors and CI require the pinned Rust toolchain.
- Language-specific repository analysis remains an adapter concern rather than a reason to move
  domain logic out of Rust.
