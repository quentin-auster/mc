# Repository registration and snapshots

`RepositoryStore` separates repository use cases from the Git and PostgreSQL adapter. Registration
creates or refreshes a mirror under an operator-configured root using the repository UUID as its
directory name. Locators and branch names are passed to Git as individual arguments and are never
interpolated into a shell command.

Snapshot creation validates the branch with Git, fetches the mirror, resolves the branch to an
immutable commit, hashes the recursive Git tree manifest with SHA-256, and persists both the commit
and digest. Later fetches do not mutate existing snapshot records.

Repository locators are untrusted administrative input. Deployments should restrict allowed
schemes, hosts, and credentials before calling this adapter; secrets should be supplied through a
credential helper rather than embedded in persisted locators.
