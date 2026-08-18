-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- Canonical D-7 execution attempts (ADR-0003). One row per exact-action
-- attempt; the (tenant_id, attempt_id) key is the durable identity. The
-- canonical transition versions follow the D-3 projection contract:
-- prepared = 1, running = 2, terminal = 3. Every transition is one
-- single-writer conditional update:
--   prepared -> running   claims the Turn's only running slot,
--   running/prepared -> terminal commits exactly one bounded outcome,
-- and every loser reads the already-committed canonical row (TC-12).
-- The partial unique index enforces the Turn's single running D-7 across
-- instances (TC-09); the CHECK constraint keeps each status shape legal as
-- defense in depth, including the 1,048,576-byte committed-output bound, a
-- non-blank stable failure code for failed terminals, and the
-- still-prepared cancellation permitted only as cancelled/not_started.

CREATE TABLE IF NOT EXISTS tool_execution_attempts (
    tenant_id TEXT NOT NULL,
    attempt_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    lease_generation BIGINT NOT NULL CHECK (lease_generation > 0),
    descriptor_id TEXT NOT NULL,
    descriptor_version TEXT NOT NULL,
    effect TEXT NOT NULL CHECK (
        effect IN (
            'read_data',
            'external_write',
            'filesystem_write',
            'process_execute',
            'network_egress',
            'credential_use',
            'unknown'
        )
    ),
    action_digest TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    profile_version TEXT NOT NULL,
    prepared_at_millis BIGINT NOT NULL CHECK (prepared_at_millis >= 0),
    status TEXT NOT NULL CHECK (
        status IN ('prepared', 'running', 'succeeded', 'failed', 'timed_out', 'cancelled')
    ),
    started_at_millis BIGINT CHECK (
        started_at_millis IS NULL OR started_at_millis >= prepared_at_millis
    ),
    effect_state TEXT CHECK (effect_state IN ('not_started', 'started', 'unknown')),
    failure_code TEXT,
    output BYTEA CHECK (octet_length(output) <= 1048576),
    terminal_at_millis BIGINT CHECK (
        terminal_at_millis IS NULL OR terminal_at_millis >= prepared_at_millis
    ),
    version BIGINT NOT NULL CHECK (version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, attempt_id),
    CHECK (
        (
            status = 'prepared'
            AND version = 1
            AND started_at_millis IS NULL
            AND effect_state IS NULL
            AND failure_code IS NULL
            AND output IS NULL
            AND terminal_at_millis IS NULL
        )
        OR (
            status = 'running'
            AND version = 2
            AND started_at_millis IS NOT NULL
            AND effect_state IS NULL
            AND failure_code IS NULL
            AND output IS NULL
            AND terminal_at_millis IS NULL
        )
        OR (
            status IN ('succeeded', 'failed', 'timed_out', 'cancelled')
            AND version = 3
            AND effect_state IS NOT NULL
            AND terminal_at_millis IS NOT NULL
            -- A terminal without a started-at timestamp is the still-prepared
            -- close, which the accepted D-7 contract permits only as
            -- cancelled with not_started effect evidence.
            AND (
                started_at_millis IS NOT NULL
                OR (status = 'cancelled' AND effect_state = 'not_started')
            )
            AND (started_at_millis IS NULL OR terminal_at_millis >= started_at_millis)
            AND (status = 'failed' OR failure_code IS NULL)
            AND (status <> 'failed' OR failure_code ~ '[^[:space:]]')
            AND (status = 'succeeded' OR output IS NULL)
            AND (status <> 'succeeded' OR output IS NOT NULL)
        )
    )
);

CREATE INDEX IF NOT EXISTS tool_execution_attempts_turn_lookup
    ON tool_execution_attempts (tenant_id, turn_id, status);

CREATE UNIQUE INDEX IF NOT EXISTS tool_execution_attempts_one_running_per_turn
    ON tool_execution_attempts (tenant_id, turn_id)
    WHERE status = 'running';
