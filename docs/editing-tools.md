# Repository editing tools

`RepositoryEditor` supports single-file unified-diff apply, line-range replacement, file creation,
deletion, and Git revert. Worktrees and paths are canonicalized beneath the configured managed root;
absolute paths, parent components, symlink escapes, headerless patches, and patches targeting a
different file are rejected.

Every successful edit stores before and after bytes as content-addressed artifacts and appends a
versioned `repository_edited` run event containing the operation, path, and artifact IDs. This makes
the mutation inspectable even after a worktree is cleaned. Artifact writes precede the event, so
failed event persistence may leave unreferenced objects for lifecycle cleanup.
