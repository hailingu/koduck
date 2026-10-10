// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-2 through AC-5: admission, concurrency, settlement, and bounds of the
//! production `SqlxPostgresExecutor` correction port against a disposable
//! production `PostgreSQL` (ADR-0004 CA-01 through CA-09).
//!
//! The binary intentionally fails when `KODUCK_AI_TEST_DATABASE_URL` is
//! missing: the isolated migrated database is a declared acceptance
//! prerequisite for AC-6 (ADR-0004 Acceptance Checks).

#[path = "postgres_cand_11/harness.rs"]
mod harness;

/// Serializes the database-bound tests of this binary: several acceptance
/// checks assert real-time budgets (AC-0's two-second proof, settlement
/// deadlines) that host-level database contention must not disturb.
pub(crate) static DATABASE_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the database test lock for one test's whole duration.
pub(crate) fn serialize_database_tests() -> std::sync::MutexGuard<'static, ()> {
    DATABASE_TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[path = "postgres_cand_11/admission_matrix.rs"]
mod admission_matrix;

#[path = "postgres_cand_11/concurrency_and_retry.rs"]
mod concurrency_and_retry;

#[path = "postgres_cand_11/settlement_and_cancellation.rs"]
mod settlement_and_cancellation;

#[path = "postgres_cand_11/bounds_and_atomicity.rs"]
mod bounds_and_atomicity;

#[path = "postgres_cand_11/retry_ancestry.rs"]
mod retry_ancestry;

#[path = "postgres_cand_11/commit_fault_probe.rs"]
mod commit_fault_probe;

#[path = "postgres_cand_11/submission.rs"]
mod submission;

/// AC-0 (ADR-0018): a delayed deferred-trigger commit survives the dropped
/// write acknowledgement and is provable on the writer's own backend within
/// the full proof budget.
#[test]
fn cand_18_commit_fault_probe_same_backend() {
    commit_fault_probe::run_same_backend();
}

/// AC-0 (ADR-0018): the proof observes the same committed marker from a
/// different released backend while the writer commit is still blocked.
#[test]
fn cand_18_commit_fault_probe_other_backend() {
    commit_fault_probe::run_other_backend();
}

/// AC-0 (ADR-0018): a deferred `RAISE EXCEPTION` leaves zero marker rows and
/// no leaked primary session.
#[test]
fn cand_18_commit_fault_probe_aborted_commit() {
    commit_fault_probe::run_aborted_commit();
}

/// AC-2/AC-4 (ADR-0018): one atomic identified acceptance, exact-retry
/// receipts with zero mutation, typed drift conflicts, independent owner
/// scopes, and preserved unidentified behavior.
#[test]
fn cand_18_atomic_acceptance() {
    submission::cand_18_atomic_acceptance();
}

/// AC-3 (ADR-0018): contended exact submissions converge on one created
/// owner and identical observation receipts.
#[test]
fn cand_18_concurrent_identity() {
    submission::cand_18_concurrent_identity();
}

/// AC-4 (ADR-0018): exact equality and owner scope stay stable across input
/// and selector drift with zero mutation on every conflict.
#[test]
fn cand_18_equality_and_scope() {
    submission::cand_18_equality_and_scope();
}

/// AC-2/AC-6/SI-04/SI-07 (ADR-0018): the production runner identified flow —
/// fresh acceptance executes the created owner, the exact retry observes,
/// and drift conflicts.
#[test]
fn cand_18_runner_flows() {
    submission::cand_18_runner_flows();
}

/// AC-6/SI-07g/SI-07h (ADR-0018): a cancellation racing the proven Created
/// outcome enters the durable cancellation path, and an observed
/// pre-acceptance cancellation wins before any write.
#[test]
fn cand_18_runner_cancellation() {
    submission::cand_18_runner_cancellation();
}

/// SI-07g (ADR-0018): the observed pre-acceptance cancellation wins before
/// any write with zero durable state.
#[test]
fn cand_18_runner_pre_acceptance_cancellation() {
    submission::cand_18_runner_pre_acceptance_cancellation();
}

/// SI-07i (ADR-0018): an earlier-than-start acceptance reading fails closed.
#[test]
fn cand_18_invalid_clock_fails_closed() {
    submission::cand_18_invalid_clock_fails_closed();
}

/// AC-8 (ADR-0018): the production startup sequence is idempotent on an
/// isolated populated fixture.
#[test]
fn cand_18_migration_and_integrity() {
    submission::cand_18_migration_and_integrity();
}

/// AC-5 (ADR-0018): the production commit-loss and abort variants reconcile
/// truthfully through the isolated deferred-trigger fixture.
#[test]
fn cand_18_commit_reconciliation() {
    submission::cand_18_commit_reconciliation();
}

/// AC-6/SI-07d (ADR-0018): the exact remaining-time write gate and the sufficient four-second boundary with real database timers.
#[test]
fn cand_18_deadline_gate() {
    submission::cand_18_deadline_gate();
}

/// AC-7/SI-07b (ADR-0018): a rejected fresh preparation resolves the final unlocked lookup and preserves the original rejection.
#[test]
fn cand_18_preparation_rejection() {
    submission::cand_18_preparation_rejection();
}

/// AC-1/SI-01d (ADR-0018): the default identified ports fail closed without any canonical effect.
#[test]
fn cand_18_default_port_fails_closed() {
    submission::cand_18_default_port_fails_closed();
}

/// AC-2 (ADR-0018): the unconfigured Tool boundary records the model call as the typed durable denial.
#[test]
fn cand_18_runner_tool_call_is_recorded_as_denial() {
    submission::cand_18_runner_tool_call_is_recorded_as_denial();
}

/// ADR-0003 TC-11 (ADR-0018 preservation): a Tool round ending its stream starts the continuation carrying the committed denial.
#[test]
fn cand_18_runner_tool_round_continuation() {
    submission::cand_18_runner_tool_round_continuation();
}

/// ADR-0005 PLB-7 (ADR-0018 preservation): a provider stream ending without a terminal closes the Turn as the bounded failure.
#[test]
fn cand_18_runner_stream_ended_without_terminal() {
    submission::cand_18_runner_stream_ended_without_terminal();
}

/// AC-9 (ADR-0018): the production run assembly executes through the explicit acceptance-clock composition to the listener bind.
#[test]
fn cand_18_runtime_assembly() {
    submission::cand_18_runtime_assembly();
}

/// AC-3/SI-03b (ADR-0018): the int4-pair submission lock namespace never
/// overlaps the bigint Item namespace, proven through live `pg_locks`.
#[test]
fn cand_18_locks_namespace_is_isolated() {
    submission::cand_18_locks_namespace_is_isolated();
}

/// AC-3/SI-03b (ADR-0018): a submission-lock waiter owns no Item lock.
#[test]
fn cand_18_submission_waiter_holds_no_item_lock() {
    submission::cand_18_submission_waiter_holds_no_item_lock();
}

/// AC-3/AC-7/SI-07a (ADR-0018): the unlocked preliminary read is not
/// serialized behind a held submission lock.
#[test]
fn cand_18_preliminary_read_unblocked_by_submission_lock() {
    submission::cand_18_preliminary_read_unblocked_by_submission_lock();
}

/// AC-3/SI-04/SI-07e (ADR-0018): an exact retry completes while the original
/// provider stream blocks, and a different key proceeds independently.
#[test]
fn cand_18_retry_completes_while_provider_blocks() {
    submission::cand_18_retry_completes_while_provider_blocks();
}

/// AC-4/SI-02b (ADR-0018): Unicode-composition and whitespace drift are the
/// typed conflict with zero mutation.
#[test]
fn cand_18_normalization_and_whitespace_drift() {
    submission::cand_18_normalization_and_whitespace_drift();
}

/// AC-4/SI-02e (ADR-0018): a fresh foreign or unknown explicit Thread is the
/// indistinguishable typed `NotFound`.
#[test]
fn cand_18_foreign_thread_not_found_indistinguishable() {
    submission::cand_18_foreign_thread_not_found_indistinguishable();
}

/// AC-4/SI-05 (ADR-0018): the receipt is constant across every lifecycle
/// status and lease expiry.
#[test]
fn cand_18_receipt_constant_across_lifecycle() {
    submission::cand_18_receipt_constant_across_lifecycle();
}

/// AC-4/SI-10 (ADR-0018): an exact retry after a post-terminal correction
/// still observes the original acceptance.
#[test]
fn cand_18_retry_after_correction() {
    submission::cand_18_retry_after_correction();
}

/// AC-5/SI-06c/SI-06d (ADR-0018): every dead-creator proof branch reconciles
/// to the observation receipt without authority.
#[test]
fn cand_18_dead_creator_settlement_branches() {
    submission::cand_18_dead_creator_settlement_branches();
}

/// AC-5/SI-04 (ADR-0018): a recreated runner handle never regains execution
/// authority for an existing key.
#[test]
fn cand_18_restart_cannot_regain_authority() {
    submission::cand_18_restart_cannot_regain_authority();
}

/// AC-8/SI-08b (ADR-0018): each migration constraint independently rejects
/// its invalid row.
#[test]
fn cand_18_binding_constraints_reject_invalid_rows() {
    submission::cand_18_binding_constraints_reject_invalid_rows();
}

/// AC-8/SI-08d (ADR-0018): the binding survives the retained-canonical
/// lifetime transitions with the identical receipt.
#[test]
fn cand_18_binding_lifetime_preserved_across_transitions() {
    submission::cand_18_binding_lifetime_preserved_across_transitions();
}

/// AC-7/SI-07f (ADR-0018): on the real router over the migrated database, a
/// disconnected retry and the creator's dropped stream isolate the owner.
#[test]
fn cand_18_stream_body_drop_isolates_owner() {
    submission::cand_18_stream_body_drop_isolates_owner();
}

/// AC-2: CA-02/CA-03 admission and CA-05/CA-09 preservation hold for every
/// Turn state, ownership dimension, Item kind, corrupt ancestor shape, and
/// stored-identity case.
#[test]
fn admission_matrix() {
    let _database_guard = serialize_database_tests();
    admission_matrix::run();
}

/// CA-03/CA-04: a matching durable identity cannot bypass ancestry validation.
#[test]
fn exact_retry_validates_ancestry() {
    let _database_guard = serialize_database_tests();
    retry_ancestry::run();
}

/// AC-3: CA-04/CA-05 concurrency and retry converge under the measured
/// timing precondition.
#[test]
fn concurrency_and_retry() {
    let _database_guard = serialize_database_tests();
    concurrency_and_retry::run();
}

/// AC-4: CA-07/CA-08 settlement is bounded and truthful under real lock,
/// deadline, and cancellation faults.
#[test]
fn settlement_and_cancellation() {
    let _database_guard = serialize_database_tests();
    settlement_and_cancellation::run();
}

/// AC-5: CA-05/CA-06/CA-08 enforce the exact bounds with zero mutation on
/// every proven rejection or rollback.
#[test]
fn bounds_and_atomicity() {
    let _database_guard = serialize_database_tests();
    bounds_and_atomicity::run();
}
