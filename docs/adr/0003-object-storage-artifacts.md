# 0003: Store large artifacts in object storage

- Status: Accepted
- Date: 2026-07-22

## Context

Tool output, patches, logs, repository bundles, and model payloads can be too large or too binary
for efficient relational storage. They still require durable identity, integrity verification, and
links to the run that produced them.

## Decision

Store large or binary artifact bodies in S3-compatible object storage. Store artifact metadata,
content hashes, media types, sizes, provenance, and object keys in PostgreSQL. Treat object keys as
internal locations rather than stable public identity; the content digest is the integrity anchor.

## Consequences

- PostgreSQL remains focused on queryable structured state.
- Artifact reads must verify identity and authorization using canonical metadata.
- Deleting or expiring objects requires a policy that preserves referenced evidence.
- MinIO provides the local S3-compatible implementation without coupling domain code to it.
