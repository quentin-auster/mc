# Run worktrees

`WorktreeManager` creates one detached Git worktree per run from the immutable commit recorded by
its repository snapshot. Mirror and worktree locations are operator-configured roots; concrete
paths are derived from repository and run UUIDs rather than user input.

Inspection records porcelain dirty paths and `git diff --binary HEAD` bytes in PostgreSQL and
returns the same structured state. Completion removes successful worktrees. Failed worktrees may
be retained explicitly for diagnosis and can later be cleaned through the same manager.

Worktree contents remain untrusted repository data. Callers must use repository path validation
and sandboxed command execution rather than treating a managed worktree as a trusted filesystem.
