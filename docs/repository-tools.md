# Repository read tools

`RepositoryReader` exposes structured file listing, bounded line reads, literal text search, Git
diff, commit history, and blame. The local adapter accepts only existing worktrees beneath its
configured root. Relative paths reject absolute and parent components, and canonicalization blocks
symlink escapes before content or Git operations run.

Search is literal rather than regex-based, so repository text cannot change query semantics. Git
arguments are passed directly without a shell. Text operations skip no traversal errors; unreadable
or malformed inputs produce explicit failures. Callers should apply result-size budgets through the
tool-policy layer before presenting repository content to a model.
