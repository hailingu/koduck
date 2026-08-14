-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- Canonical D-6 approval records (ADR-0003). One row per exact-action
-- approval request; the (tenant_id, approval_id) key is the durable identity.
-- Decision transitions are single-writer conditional updates:
-- UPDATE ... WHERE status = 'requested' AND expires_at_millis > decided_at
-- commits exactly one terminal decision and increments the record version;
-- every loser reads the already-committed terminal. A decision at or after
-- expiry commits no decision and conditionally transitions the still-
-- requested record to 'expired'. The authenticated requester subject column
-- lands with the T-2 approval transport, which owns the caller identity.

CREATE TABLE IF NOT EXISTS tool_approvals (
    tenant_id TEXT NOT NULL,
    approval_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    attempt_id UUID NOT NULL,
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
    requested_at_millis BIGINT NOT NULL CHECK (requested_at_millis >= 0),
    expires_at_millis BIGINT NOT NULL CHECK (expires_at_millis > requested_at_millis),
    status TEXT NOT NULL CHECK (
        status IN ('requested', 'accepted', 'declined', 'cancelled', 'expired')
    ),
    decision TEXT CHECK (decision IN ('accepted', 'declined', 'cancelled')),
    approver TEXT,
    decided_at_millis BIGINT CHECK (
        decided_at_millis IS NULL OR decided_at_millis >= requested_at_millis
    ),
    version BIGINT NOT NULL CHECK (version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, approval_id),
    CHECK (
        (
            status IN ('requested', 'expired')
            AND decision IS NULL
            AND approver IS NULL
            AND decided_at_millis IS NULL
        )
        OR (
            status = decision
            AND decision IS NOT NULL
            AND approver IS NOT NULL
            AND approver ~ '[^[:space:]]'
            AND decided_at_millis IS NOT NULL
            AND decided_at_millis < expires_at_millis
        )
    )
);

CREATE INDEX IF NOT EXISTS tool_approvals_pending_lookup
    ON tool_approvals (tenant_id, thread_id, status, expires_at_millis);
