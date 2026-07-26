# Artifact store

`mc-application::ArtifactStore` accepts and returns asynchronous byte streams. The PostgreSQL/S3
adapter hashes uploads while spooling them to a temporary file, then performs an 8 MiB multipart
upload to a content-addressed `sha256/<prefix>/<digest>` key. Multiple artifact metadata records
may reference that same immutable object, providing SHA-256 deduplication without losing per-run
provenance.

Metadata remains canonical in PostgreSQL. Reads resolve the authorized artifact identity through
metadata before opening the object, and callers can stream the full body or request a validated,
half-open byte range. This makes large text logs inspectable without downloading the entire object.

Object keys and bucket credentials are infrastructure details. Authorization must be checked
before calling the store; raw object endpoints and credentials must not be exposed to model or
repository-controlled content.
