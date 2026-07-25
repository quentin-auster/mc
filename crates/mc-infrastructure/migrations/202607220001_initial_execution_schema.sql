CREATE TABLE repositories (
    id UUID PRIMARY KEY,
    locator TEXT NOT NULL CHECK (btrim(locator) <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE repository_snapshots (
    id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repositories(id),
    parent_id UUID,
    source_revision TEXT,
    tree_digest CHAR(64) NOT NULL CHECK (tree_digest ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, repository_id),
    FOREIGN KEY (parent_id, repository_id)
        REFERENCES repository_snapshots(id, repository_id)
);

CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repositories(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, repository_id)
);

CREATE TABLE task_contracts (
    task_id UUID NOT NULL REFERENCES tasks(id),
    version INTEGER NOT NULL CHECK (version > 0),
    objective TEXT NOT NULL CHECK (btrim(objective) <> ''),
    constraints JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(constraints) = 'array'),
    acceptance_criteria JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(acceptance_criteria) = 'array'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, version)
);

CREATE TABLE runs (
    id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES repositories(id),
    task_id UUID NOT NULL,
    task_contract_version INTEGER NOT NULL CHECK (task_contract_version > 0),
    snapshot_id UUID NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'running', 'awaiting_approval', 'succeeded', 'failed', 'cancelled')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, repository_id),
    FOREIGN KEY (task_id, repository_id) REFERENCES tasks(id, repository_id),
    FOREIGN KEY (task_id, task_contract_version) REFERENCES task_contracts(task_id, version),
    FOREIGN KEY (snapshot_id, repository_id)
        REFERENCES repository_snapshots(id, repository_id)
);

CREATE TABLE run_events (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    occurred_at TIMESTAMPTZ NOT NULL,
    causation_id UUID,
    correlation_id TEXT,
    kind TEXT NOT NULL CHECK (btrim(kind) <> ''),
    payload_version INTEGER NOT NULL CHECK (payload_version > 0),
    payload JSONB NOT NULL,
    UNIQUE (run_id, sequence),
    UNIQUE (id, run_id),
    FOREIGN KEY (causation_id, run_id) REFERENCES run_events(id, run_id)
);

CREATE TABLE artifacts (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    kind TEXT NOT NULL CHECK (btrim(kind) <> ''),
    digest CHAR(64) NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    media_type TEXT NOT NULL CHECK (btrim(media_type) <> ''),
    byte_length BIGINT NOT NULL CHECK (byte_length >= 0),
    object_key TEXT NOT NULL CHECK (btrim(object_key) <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, run_id),
    UNIQUE (object_key)
);

CREATE TABLE tool_calls (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    requested_by UUID NOT NULL,
    tool_name TEXT NOT NULL CHECK (btrim(tool_name) <> ''),
    arguments_artifact_id UUID NOT NULL,
    result_artifact_id UUID,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'awaiting_approval', 'running', 'succeeded', 'failed', 'denied')
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (requested_by, run_id) REFERENCES run_events(id, run_id),
    FOREIGN KEY (arguments_artifact_id, run_id) REFERENCES artifacts(id, run_id),
    FOREIGN KEY (result_artifact_id, run_id) REFERENCES artifacts(id, run_id)
);

CREATE TABLE context_snapshots (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    rendered_digest CHAR(64) NOT NULL CHECK (rendered_digest ~ '^[0-9a-f]{64}$'),
    estimated_tokens BIGINT NOT NULL CHECK (estimated_tokens >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, run_id)
);

CREATE TABLE context_items (
    id UUID PRIMARY KEY,
    context_snapshot_id UUID NOT NULL REFERENCES context_snapshots(id),
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    kind TEXT NOT NULL CHECK (btrim(kind) <> ''),
    source JSONB NOT NULL CHECK (jsonb_typeof(source) = 'object'),
    inclusion_reason TEXT NOT NULL CHECK (btrim(inclusion_reason) <> ''),
    estimated_tokens BIGINT NOT NULL CHECK (estimated_tokens >= 0),
    rendered_digest CHAR(64) NOT NULL CHECK (rendered_digest ~ '^[0-9a-f]{64}$'),
    compacted_from UUID[] NOT NULL DEFAULT '{}',
    UNIQUE (context_snapshot_id, sequence)
);

CREATE TABLE model_invocations (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id),
    context_snapshot_id UUID NOT NULL,
    provider TEXT NOT NULL CHECK (btrim(provider) <> ''),
    model TEXT NOT NULL CHECK (btrim(model) <> ''),
    request_artifact_id UUID NOT NULL,
    response_artifact_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (context_snapshot_id, run_id) REFERENCES context_snapshots(id, run_id),
    FOREIGN KEY (request_artifact_id, run_id) REFERENCES artifacts(id, run_id),
    FOREIGN KEY (response_artifact_id, run_id) REFERENCES artifacts(id, run_id)
);

CREATE INDEX repository_snapshots_repository_created_idx
    ON repository_snapshots(repository_id, created_at DESC);
CREATE INDEX runs_task_created_idx ON runs(task_id, created_at DESC);
CREATE INDEX run_events_run_sequence_idx ON run_events(run_id, sequence);
CREATE INDEX artifacts_run_created_idx ON artifacts(run_id, created_at);
CREATE INDEX tool_calls_run_created_idx ON tool_calls(run_id, created_at);
CREATE INDEX context_snapshots_run_created_idx ON context_snapshots(run_id, created_at);
CREATE INDEX model_invocations_run_created_idx ON model_invocations(run_id, created_at);
