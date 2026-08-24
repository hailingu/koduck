-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- Canonical append-only C-5 audit trail (ADR-0003 TC-14): one row per
-- serialized, bounded, correlated audit terminal emitted by C-5 — every
-- default-deny policy decision, every canonical D-6 resolution, and every
-- D-7 execution terminal, including pre-driver denials resolved before the
-- C-5 driver allocates any D-6/D-7. The application enforces the
-- 16,384-byte serialization bound before any sink is consulted; the
-- octet_length CHECK is defense in depth and counts bytes, not characters,
-- because a multibyte record within 16,384 characters can still occupy far
-- more than 16,384 bytes. Rows are append-only evidence: nothing in this
-- slice reads or rewrites them, and the Turn-correlation index serves the
-- operator and reconciliation lookups that observe missing audit evidence.

CREATE TABLE IF NOT EXISTS tool_audit_records (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    at_millis BIGINT NOT NULL CHECK (at_millis >= 0),
    record TEXT NOT NULL CHECK (octet_length(record) BETWEEN 1 AND 16384),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS tool_audit_records_turn_lookup
    ON tool_audit_records (tenant_id, thread_id, turn_id, id);
