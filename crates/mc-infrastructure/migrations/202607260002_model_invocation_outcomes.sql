ALTER TABLE model_invocations
    ADD COLUMN status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'succeeded', 'failed')),
    ADD COLUMN input_tokens BIGINT CHECK (input_tokens >= 0),
    ADD COLUMN output_tokens BIGINT CHECK (output_tokens >= 0),
    ADD COLUMN latency_milliseconds BIGINT CHECK (latency_milliseconds >= 0),
    ADD COLUMN provider_request_id TEXT,
    ADD COLUMN error_code TEXT,
    ADD COLUMN error_message TEXT,
    ADD COLUMN completed_at TIMESTAMPTZ,
    ADD CONSTRAINT model_invocations_terminal_shape CHECK (
        (status = 'running'
            AND response_artifact_id IS NULL
            AND input_tokens IS NULL
            AND output_tokens IS NULL
            AND latency_milliseconds IS NULL
            AND error_code IS NULL
            AND error_message IS NULL
            AND completed_at IS NULL)
        OR
        (status = 'succeeded'
            AND response_artifact_id IS NOT NULL
            AND input_tokens IS NOT NULL
            AND output_tokens IS NOT NULL
            AND latency_milliseconds IS NOT NULL
            AND error_code IS NULL
            AND error_message IS NULL
            AND completed_at IS NOT NULL)
        OR
        (status = 'failed'
            AND response_artifact_id IS NOT NULL
            AND latency_milliseconds IS NOT NULL
            AND error_message IS NOT NULL
            AND completed_at IS NOT NULL)
    );
