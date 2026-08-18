-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- Durable interruption barrier for canonical D-7 preparation and dispatch.
-- The existing interrupt_requested flag remains the CAND-1 terminal/publication
-- signal; this separate transient flag closes the cross-instance gap before
-- C-5 has cancelled or reconciled every live D-7.

ALTER TABLE turns
    ADD COLUMN IF NOT EXISTS interrupting BOOLEAN NOT NULL DEFAULT FALSE;
