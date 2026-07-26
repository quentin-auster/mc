CREATE TABLE tool_policy_decisions (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    role TEXT NOT NULL CHECK (btrim(role) <> ''),
    action JSONB NOT NULL CHECK (jsonb_typeof(action) = 'object'),
    allowed BOOLEAN NOT NULL,
    denial_reason TEXT,
    remaining_calls BIGINT NOT NULL CHECK (remaining_calls >= 0),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    decided_at TIMESTAMPTZ NOT NULL,
    CHECK ((allowed AND denial_reason IS NULL) OR (NOT allowed AND denial_reason IS NOT NULL))
);

CREATE INDEX tool_policy_decisions_run_decided_idx
    ON tool_policy_decisions(run_id, decided_at);
