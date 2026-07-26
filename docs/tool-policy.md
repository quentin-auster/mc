# Tool policy

The versioned `ToolPolicyEngine` authorizes actions by run and role. Role policies independently
control read/edit path prefixes, typed command argv prefixes, network hosts, secret names, and a
maximum number of allowed calls. Rules are default-deny; paths reject absolute and parent
components, and denied requests do not consume budget.

The PostgreSQL adapter serializes authorization for each run/role with a transaction-level advisory
lock, counts prior allowed decisions, and stores every allow or deny with policy version, remaining
budget, action, and reason. This keeps budgets consistent across API and worker processes and across
restarts.

Secret actions contain only the configured secret name. Secret values must be injected after an
allow decision by a separate credential boundary and must never enter policy requests, audit rows,
events, or tracing fields.
