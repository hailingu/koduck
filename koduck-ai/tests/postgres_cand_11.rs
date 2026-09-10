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

/// AC-2: CA-02/CA-03 admission and CA-05/CA-09 preservation hold for every
/// Turn state, ownership dimension, Item kind, corrupt ancestor shape, and
/// stored-identity case.
#[test]
fn admission_matrix() {
    admission_matrix::run();
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
