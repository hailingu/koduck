-- ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md
--
-- CAND-18 durable subject-scoped submission identity (SI-08a/SI-08b).
--
-- The migration is additive and idempotent: every statement uses IF NOT
-- EXISTS semantics, no existing row or constraint of migrations 0001..0009
-- changes, and no historical Turn receives a guessed identity (SI-10).
-- A Turn owns at most one binding; a binding can never outlive its accepted
-- canonical Turn through the foreign key.

CREATE TABLE IF NOT EXISTS chat_submissions (
    tenant_id TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    submission_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    turn_id UUID NOT NULL,
    original_thread_id UUID,
    creator_operation_id UUID NOT NULL,
    PRIMARY KEY (tenant_id, subject_id, submission_id),
    CONSTRAINT chat_submissions_turn_unique UNIQUE (tenant_id, thread_id, turn_id),
    CONSTRAINT chat_submissions_turn_fk FOREIGN KEY (tenant_id, thread_id, turn_id)
        REFERENCES turns (tenant_id, thread_id, turn_id),
    CONSTRAINT chat_submissions_original_selector_check CHECK (
        original_thread_id IS NULL OR original_thread_id = thread_id),
    CONSTRAINT chat_submissions_non_nil_submission_id CHECK (
        submission_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT chat_submissions_non_nil_creator_operation CHECK (
        creator_operation_id <> '00000000-0000-0000-0000-000000000000'::uuid)
);
