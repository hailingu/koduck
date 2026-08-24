-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- An authenticated Turn interruption is a distinct D-6 cancellation owner:
-- it closes a requested approval but is not a C-7 approval decision, so it
-- records neither an approver nor a decision. Existing `cancelled` rows with
-- a C-7 cancellation decision remain valid under the second branch below.

DO $$
DECLARE
    constraint_name TEXT;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'tool_approvals'::regclass
          AND conname = 'tool_approvals_resolution_shape'
    ) THEN
        RETURN;
    END IF;
    FOR constraint_name IN
        SELECT conname
        FROM pg_constraint
        WHERE conrelid = 'tool_approvals'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%decision IS NULL%'
    LOOP
        EXECUTE format(
            'ALTER TABLE tool_approvals DROP CONSTRAINT %I',
            constraint_name
        );
    END LOOP;
    ALTER TABLE tool_approvals
        ADD CONSTRAINT tool_approvals_resolution_shape
        CHECK (
            (
                status IN ('requested', 'expired', 'cancelled')
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
        );
END $$;
