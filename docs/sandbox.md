# Container sandbox

`Sandbox` accepts a run ID, a managed worktree, an argv vector, and explicit environment values.
The Docker adapter uses an operator-selected image and fixed CPU, memory, process, and wall-clock
limits. Containers run with networking disabled, all capabilities dropped, and
`no-new-privileges`; only the validated run worktree is mounted.

The adapter does not inherit host environment variables. Callers may supply only names configured
in the adapter allowlist. Commands are passed as argv entries without a host shell. Standard output
and error are stored as content-addressed log artifacts, including a structured timeout result.

Container images are part of the execution policy and should be pinned by immutable digest in
production. Docker daemon access is privileged infrastructure and must remain outside untrusted
tool inputs.
