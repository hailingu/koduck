-- ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

CREATE TABLE IF NOT EXISTS threads (
    tenant_id TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    thread_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, thread_id)
);

CREATE TABLE IF NOT EXISTS turns (
    tenant_id TEXT NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'started',
            'recovery-pending',
            'completed',
            'failed',
            'interrupted',
            'cancelled'
        )
    ),
    next_sequence BIGINT NOT NULL CHECK (next_sequence > 0),
    interrupt_requested BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, thread_id, turn_id),
    UNIQUE (tenant_id, turn_id),
    FOREIGN KEY (tenant_id, thread_id)
        REFERENCES threads (tenant_id, thread_id)
);

CREATE TABLE IF NOT EXISTS turn_items (
    tenant_id TEXT NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    item_id UUID NOT NULL,
    item_type TEXT NOT NULL CHECK (
        item_type IN (
            'user_message',
            'agent_message_delta',
            'usage',
            'completed',
            'failed',
            'interrupted',
            'cancelled'
        )
    ),
    payload TEXT NOT NULL,
    is_terminal BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, thread_id, turn_id, sequence),
    UNIQUE (tenant_id, item_id),
    FOREIGN KEY (tenant_id, thread_id, turn_id)
        REFERENCES turns (tenant_id, thread_id, turn_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS turn_items_one_terminal_per_turn
    ON turn_items (tenant_id, thread_id, turn_id)
    WHERE is_terminal;

CREATE INDEX IF NOT EXISTS turn_items_thread_replay
    ON turn_items (tenant_id, thread_id, created_at, turn_id, sequence);

CREATE TABLE IF NOT EXISTS turn_leases (
    tenant_id TEXT NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    generation BIGINT NOT NULL CHECK (generation > 0),
    renewed_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    fenced BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, thread_id, turn_id),
    UNIQUE (tenant_id, thread_id, turn_id, generation),
    CHECK (expires_at >= renewed_at),
    FOREIGN KEY (tenant_id, thread_id, turn_id)
        REFERENCES turns (tenant_id, thread_id, turn_id)
);

-- Initial acceptance inserts Thread, Turn, input Item, and generation 1 lease
-- in one transaction. Append locks the matching Turn/lease key, rejects a
-- mismatched or fenced generation, allocates next_sequence, inserts one Item,
-- and advances turns.next_sequence before commit. Reconciliation locks the
-- complete tenant/Thread/Turn/generation key and conditionally marks the lease
-- fenced plus inserts one cancelled terminal in the same transaction.
