# Run-state projection

`mc-application::project_run` deterministically folds an ordered event stream into current run
working state: phase, goal, plan, active files, decisions, hypotheses, validation, and unresolved
issues.

Projection event kinds use the `run.*` namespace and payload version 1. Sequence gaps, mixed run
identities, malformed known payloads, and unsupported known-event versions fail projection.
Unrelated event kinds are ignored but still advance the sequence cursor, allowing this projection
to share the canonical stream with tools and model invocations.

The projection is derived state, not authority. It can always be rebuilt from append-only events;
callers must not edit a projection as a substitute for appending a correction event.
