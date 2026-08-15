-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

-- Forward migration: canonical requester ownership for D-6 decisions.
-- Adds the authenticated requester subject and constrains it to the same
-- non-whitespace predicate as the approver column: a blank or whitespace-only
-- subject is unresolvable by any valid principal (`TrustContext::new` rejects
-- trim-empty subjects), so the schema rejects it as defense in depth.
-- Existing rows (pre-deployment greenfield schema holds none; the backfill
-- keeps the migration safe if any environment applied version 2) are
-- backfilled with the real owner from the Thread identity: a D-6 belongs to
-- the subject who owns its Thread in the same tenant. Rows without a matching
-- Thread owner — or whose Thread owner is itself blank or whitespace-only —
-- are never assigned a placeholder subject: a fabricated owner would hand
-- legacy pending approvals to whoever holds that subject, so the migration
-- fails loudly and leaves them for operator resolution instead (TC-05/TC-12).
-- Decision lookups remain conditional on tenant plus requester subject and
-- the canonical Thread.

ALTER TABLE tool_approvals
    ADD COLUMN IF NOT EXISTS requester_subject TEXT NOT NULL DEFAULT '';

UPDATE tool_approvals AS approval
SET requester_subject = owner.subject_id
FROM threads AS owner
WHERE approval.requester_subject = ''
    AND approval.tenant_id = owner.tenant_id
    AND approval.thread_id = owner.thread_id;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM tool_approvals WHERE requester_subject !~ '[^[:space:]]') THEN
        RAISE EXCEPTION 'tool_approvals contains row(s) with no resolvable threads owner; resolve their requester ownership before applying requester-ownership enforcement';
    END IF;
END $$;

ALTER TABLE tool_approvals
    DROP CONSTRAINT IF EXISTS tool_approvals_requester_subject_nonblank;
ALTER TABLE tool_approvals
    ADD CONSTRAINT tool_approvals_requester_subject_nonblank
    CHECK (requester_subject ~ '[^[:space:]]');
