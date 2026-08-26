-- ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md
--
-- Additive CAND-3 correction Item schema: the durable correction
-- relationship column, its structural constraints, and the 'correction'
-- discriminator. Forward-only and idempotent; migrations 0001 through 0008
-- stay untouched and every existing row and constraint is preserved.

ALTER TABLE turn_items ADD COLUMN IF NOT EXISTS corrects_item_id UUID;

-- Referenced tuple for the same-Turn composite foreign key. item_id is
-- already unique per tenant, so this unique index adds no new row constraint.
CREATE UNIQUE INDEX IF NOT EXISTS turn_items_turn_item_identity
    ON turn_items (tenant_id, thread_id, turn_id, item_id);

-- At most one direct correction successor per predecessor (CR-03).
CREATE UNIQUE INDEX IF NOT EXISTS turn_items_one_direct_correction
    ON turn_items (tenant_id, corrects_item_id) WHERE corrects_item_id IS NOT NULL;

DO $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('koduck.turn_items.correction.v1'));

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'turn_items'::regclass
          AND conname = 'turn_items_item_type_check'
          AND pg_get_constraintdef(oid) LIKE '%correction%'
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
                    'tool_result',
                    'correction'
                )
            );
    END IF;

    -- The relationship is present exactly on a correction row, and a
    -- correction is never the Turn terminal (CR-01/CR-02).
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'turn_items'::regclass
          AND conname = 'turn_items_correction_shape'
    ) THEN
        ALTER TABLE turn_items
            ADD CONSTRAINT turn_items_correction_shape CHECK (
                (
                    corrects_item_id IS NOT NULL
                    AND item_type = 'correction'
                    AND NOT is_terminal
                )
                OR (corrects_item_id IS NULL AND item_type <> 'correction')
            );
    END IF;

    -- A correcting Item must not identify itself (CR-02).
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'turn_items'::regclass
          AND conname = 'turn_items_correction_not_self'
    ) THEN
        ALTER TABLE turn_items
            ADD CONSTRAINT turn_items_correction_not_self CHECK (
                corrects_item_id IS NULL OR corrects_item_id <> item_id
            );
    END IF;

    -- The predecessor must be an existing Item in the same tenant, Thread,
    -- and Turn (CR-02).
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'turn_items'::regclass
          AND conname = 'turn_items_correction_scope'
          AND contype = 'f'
    ) THEN
        ALTER TABLE turn_items
            ADD CONSTRAINT turn_items_correction_scope FOREIGN KEY (
                tenant_id, thread_id, turn_id, corrects_item_id
            ) REFERENCES turn_items (tenant_id, thread_id, turn_id, item_id);
    END IF;
END $$;
