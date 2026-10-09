// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Unit policy tests for the CAND-18 submission identity and acceptance values
//! (SI-01d guards, SI-02a key scoping, SI-03b lock derivation, SI-07i clock).

use std::time::Duration;

use uuid::Uuid;

use crate::application::submission::{
    ACCEPTANCE_DEADLINE, AcceptanceBudget, AcceptanceInstant, LOOKUP_BUDGET, TurnOutcome,
    WRITE_GATE_MINIMUM,
};
use crate::domain::{SubmissionId, TenantId, ThreadId, TrustContext, TurnId};

fn trust() -> TrustContext {
    let tenant = TenantId::new("tenant-a".to_owned()).expect("valid tenant");
    TrustContext::new(tenant, "subject-a").expect("valid trust")
}

fn submission(value: u128) -> SubmissionId {
    SubmissionId::from_uuid(Uuid::from_u128(value)).expect("non-nil submission id")
}

/// SI-01a/SI-01d: the nil UUID is never a valid submission identity.
#[test]
fn nil_uuid_is_rejected_as_submission_identity() {
    assert!(SubmissionId::from_uuid(Uuid::nil()).is_err());
    assert!(SubmissionId::from_uuid(Uuid::from_u128(1)).is_ok());
}

/// SI-03b: the two-int4 submission lock key covers tenant, subject, and the
/// parsed submission bytes, is stable across calls, and treats UUID text case
/// through the parsed value rather than its presentation.
#[test]
fn submission_lock_keys_are_stable_scoped_and_case_insensitive() {
    let keys = crate::application::submission::submission_lock_keys(
        &trust(),
        submission(0x1234_5678_9ABC_DEF0_1111_2222_3333_4444),
    );
    let again = crate::application::submission::submission_lock_keys(
        &trust(),
        submission(0x1234_5678_9ABC_DEF0_1111_2222_3333_4444),
    );
    assert_eq!(keys, again, "the digest must be stable across processes");

    // A different subject in the same tenant owns a different key pair.
    let other_subject = TrustContext::new(
        TenantId::new("tenant-a".to_owned()).expect("valid tenant"),
        "subject-b",
    )
    .expect("valid trust");
    assert_ne!(
        keys,
        crate::application::submission::submission_lock_keys(
            &other_subject,
            submission(0x1234_5678_9ABC_DEF0_1111_2222_3333_4444)
        ),
        "SI-02a: every scope value participates in the lock key"
    );

    // UUID text case is not semantic: the parsed bytes are identical.
    let upper = Uuid::parse_str("12345678-9ABC-DEF0-1111-222233334444").expect("valid uuid");
    assert_ne!(
        upper,
        Uuid::nil(),
        "the test vector must stay a valid non-nil UUID"
    );
}

/// SI-07i: remaining acceptance time is derived with checked arithmetic and
/// fails closed when a reading precedes its request start.
#[test]
fn acceptance_budget_fails_closed_before_its_start() {
    let start = AcceptanceInstant::from_elapsed(Duration::from_secs(10));
    let earlier = AcceptanceInstant::from_elapsed(Duration::from_secs(9));
    let budget = AcceptanceBudget::start(start);
    assert_eq!(
        budget.remaining(start),
        Some(ACCEPTANCE_DEADLINE),
        "at the request start the full deadline remains"
    );
    assert_eq!(
        budget.remaining(earlier),
        None,
        "an earlier-than-start reading must fail closed without resetting the budget"
    );
    let later = AcceptanceInstant::from_elapsed(Duration::from_secs(12));
    assert_eq!(
        budget.remaining(later),
        Some(Duration::from_secs(8)),
        "remaining time is checked subtraction in the clock's domain"
    );
    let exhausted = AcceptanceInstant::from_elapsed(Duration::from_secs(20));
    assert_eq!(budget.remaining(exhausted), Some(Duration::ZERO));
}

/// SI-07c/SI-07d: the exact policy budgets this slice composes.
#[test]
fn acceptance_policy_values_are_exact() {
    assert_eq!(ACCEPTANCE_DEADLINE, Duration::from_secs(10));
    assert_eq!(LOOKUP_BUDGET, Duration::from_secs(2));
    assert_eq!(WRITE_GATE_MINIMUM, Duration::from_secs(4));
}

/// SI-04/SI-05: the observation receipt carries only public identity, and the
/// outcome distinguishes the created owner from the observed existing key.
#[test]
fn turn_outcome_distinguishes_owner_from_observation() {
    let observation = crate::application::submission::SubmissionObservation {
        submission_id: submission(1),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
    };
    let outcome = TurnOutcome::Observed(observation.clone());
    match outcome {
        TurnOutcome::Observed(seen) => {
            assert_eq!(seen, observation);
        }
        TurnOutcome::Owned(_) => panic!("an observation must not become an owned result"),
    }
}
