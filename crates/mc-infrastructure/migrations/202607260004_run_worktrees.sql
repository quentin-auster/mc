CREATE TABLE run_worktrees (
    run_id UUID PRIMARY KEY REFERENCES runs(id),
    snapshot_id UUID NOT NULL REFERENCES repository_snapshots(id),
    path TEXT NOT NULL CHECK (btrim(path) <> ''),
    status TEXT NOT NULL CHECK (status IN ('active', 'retained', 'cleaned')),
    dirty_files TEXT[] NOT NULL DEFAULT '{}',
    diff BYTEA NOT NULL DEFAULT ''::bytea,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
