-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
--
-- D-3 Tool projection views are non-terminal turn items. This forward-only
-- migration extends the original CAND-1 discriminator without rewriting its
-- immutable migration history.

DO $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('koduck.turn_items.item_type.v2'));
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'turn_items'::regclass
          AND conname = 'turn_items_item_type_check'
          AND pg_get_constraintdef(oid) LIKE '%approval_status%'
          AND pg_get_constraintdef(oid) LIKE '%tool_call%'
          AND pg_get_constraintdef(oid) LIKE '%tool_result%'
    ) THEN
        ALTER TABLE turn_items
            DROP CONSTRAINT IF EXISTS turn_items_item_type_check;

        ALTER TABLE turn_items
            ADD CONSTRAINT turn_items_item_type_check CHECK (
                item_type IN (
                    'user_message',
                    'agent_message_delta',
                    'usage',
                    'completed',
                    'failed',
                    'interrupted',
                    'cancelled',
                    'approval_status',
                    'tool_call',
                    'tool_result'
                )
            );
    END IF;
END $$;
