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

/// AC-2: CA-02/CA-03 admission and CA-05/CA-09 preservation hold for every
/// Turn state, ownership dimension, Item kind, corrupt ancestor shape, and
/// stored-identity case.
#[test]
fn admission_matrix() {
    admission_matrix::run();
}

/// CA-03/CA-04: a matching durable identity cannot bypass ancestry validation.
#[test]
fn exact_retry_validates_ancestry() {
    retry_ancestry::run();
}

/// AC-3: CA-04/CA-05 concurrency and retry converge under the measured
/// timing precondition.
#[test]
fn concurrency_and_retry() {
    concurrency_and_retry::run();
}

/// AC-4: CA-07/CA-08 settlement is bounded and truthful under real lock,
/// deadline, and cancellation faults.
#[test]
fn settlement_and_cancellation() {
    settlement_and_cancellation::run();
}

/// AC-5: CA-05/CA-06/CA-08 enforce the exact bounds with zero mutation on
/// every proven rejection or rollback.
#[test]
fn bounds_and_atomicity() {
    bounds_and_atomicity::run();
}
