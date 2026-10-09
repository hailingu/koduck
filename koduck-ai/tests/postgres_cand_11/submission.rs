// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 identified acceptance against the real migrated `PostgreSQL`
//! through the production history port (AC-2, AC-3, AC-4): one atomic
//! binding, exact-retry receipts with zero mutation, typed drift conflicts,
//! independent owner scopes, and concurrent creator selection.

use koduck_ai::adapters::history::postgres::PostgresTurnHistory;
use koduck_ai::application::{HistoryError, IdentifiedAcceptance, TurnCommand, TurnHistory};
use koduck_ai::domain::{SubmissionId, TenantId, TrustContext};
use uuid::Uuid;

use super::harness::{Harness, MIGRATIONS, MIGRATIONS_ONCE};

fn connected_history() -> (
    Harness,
    PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    String,
) {
    let harness = Harness::connect(4);
    MIGRATIONS_ONCE.call_once(|| {
        harness.runtime.block_on(async {
            for migration in MIGRATIONS {
                sqlx::raw_sql(migration)
                    .execute(&harness.pool)
                    .await
                    .expect("apply production migration");
            }
        });
    });
    let history = PostgresTurnHistory::new(harness.executor());
    let tenant = format!("cand18-{}", Uuid::new_v4());
    (harness, history, tenant)
}

fn trust(tenant: &str, subject: &str) -> TrustContext {
    TrustContext::new(
        TenantId::new(tenant.to_owned()).expect("valid tenant"),
        subject,
    )
    .expect("valid trust")
}

fn identified_command(
    tenant: &str,
    subject: &str,
    submission: Uuid,
    thread: Option<koduck_ai::domain::ThreadId>,
    input: &str,
) -> TurnCommand {
    TurnCommand::new(trust(tenant, subject), thread, input.to_owned())
        .expect("valid input")
        .with_submission_id(SubmissionId::from_uuid(submission).expect("non-nil"))
        .expect("valid submission id")
}

/// Counts the durable rows one identified acceptance must produce.
async fn count_rows(
    pool: &sqlx::PgPool,
    tenant: &str,
    subject: &str,
    submission: Uuid,
) -> (i64, i64, i64, i64) {
    let binding: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_submissions \
         WHERE tenant_id = $1 AND subject_id = $2 AND submission_id = $3",
    )
    .bind(tenant)
    .bind(subject)
    .bind(submission)
    .fetch_one(pool)
    .await
    .expect("count bindings");
    let turn: (i64, i64, i64) = sqlx::query_as(
        "SELECT \
         (SELECT count(*) FROM turns t \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3), \
         (SELECT count(*) FROM turn_items i \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3 \
          AND i.sequence = 1 AND i.item_type = 'user_message'), \
         (SELECT count(*) FROM turn_leases l \
          JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
          WHERE s.tenant_id = $1 AND s.subject_id = $2 AND s.submission_id = $3 \
          AND l.generation = 1 AND NOT l.fenced)",
    )
    .bind(tenant)
    .bind(subject)
    .bind(submission)
    .fetch_one(pool)
    .await
    .expect("count canonical rows");
    (binding, turn.0, turn.1, turn.2)
}

/// AC-2/SI-03a/SI-04: one identified acceptance produces exactly one binding,
/// Turn, sequence-1 input, and live generation-1 lease; the exact retry
/// observes the same identity with zero additional mutation; unidentified
/// calls keep the existing behavior and write no binding.
#[test]
pub(crate) fn cand_18_atomic_acceptance() {
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "first input");
    let created = TurnHistory::accept_initial_with_submission(
        &mut history,
        &command,
        std::time::Duration::from_secs(2),
    )
    .expect("the first invocation creates");
    let IdentifiedAcceptance::Created(accepted) = &created else {
        panic!("the fresh key must return the created owner, got {created:?}");
    };
    let snapshot =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(
        snapshot,
        (1, 1, 1, 1),
        "one binding, one Turn, one sequence-1 input, one live lease"
    );
    assert_eq!(
        accepted.generation.get(),
        1,
        "the created owner carries the initial lease generation"
    );

    // Exact retry: observation receipt, zero mutation.
    let retry = TurnHistory::accept_initial_with_submission(
        &mut history,
        &command,
        std::time::Duration::from_secs(2),
    )
    .expect("the exact retry resolves");
    let IdentifiedAcceptance::Existing(receipt) = &retry else {
        panic!("the exact retry must observe, got {retry:?}");
    };
    assert_eq!(
        receipt.submission_id.as_uuid(),
        submission,
        "the receipt carries the client's submission identity"
    );
    assert_eq!(receipt.thread_id, accepted.thread_id);
    assert_eq!(receipt.turn_id, accepted.turn_id);
    let after =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(snapshot, after, "a retry performs no mutation");

    // Unidentified requests keep the existing behavior and write no binding
    // (SI-10): distinct Turns, no binding rows for their identities.
    let legacy_a = TurnCommand::new(trust(&tenant, "subject-a"), None, "legacy input".to_owned())
        .expect("valid legacy command");
    let legacy_b = TurnCommand::new(trust(&tenant, "subject-a"), None, "legacy input".to_owned())
        .expect("valid legacy command");
    let accepted_a =
        TurnHistory::accept_initial(&mut history, &legacy_a).expect("legacy acceptance");
    let accepted_b =
        TurnHistory::accept_initial(&mut history, &legacy_b).expect("second legacy acceptance");
    assert_ne!(
        accepted_a.turn_id, accepted_b.turn_id,
        "unidentified calls still create fresh Turns"
    );
    let bindings: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM chat_submissions WHERE tenant_id = $1")
            .bind(&tenant)
            .fetch_one(&harness.pool)
            .await
            .expect("count tenant bindings")
    });
    assert_eq!(
        bindings, 1,
        "only the identified binding exists; legacy calls write none"
    );
}

/// AC-4/SI-02a/SI-02b/SI-02d/SI-02f: changed semantic input or selector under
/// an owned key is the typed conflict with zero mutation, and the same UUID
/// under another subject owns an independent key.
#[test]
pub(crate) fn cand_18_equality_and_scope() {
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "first input");
    let created = TurnHistory::accept_initial_with_submission(
        &mut history,
        &command,
        std::time::Duration::from_secs(2),
    )
    .expect("the first invocation creates");
    let IdentifiedAcceptance::Created(accepted) = &created else {
        panic!("the fresh key must return the created owner, got {created:?}");
    };
    let snapshot =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(snapshot, (1, 1, 1, 1));

    // Drifted semantic input under the owned key: the typed conflict.
    let drifted = identified_command(&tenant, "subject-a", submission, None, "changed input");
    assert_eq!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &drifted,
            std::time::Duration::from_secs(2),
        )
        .expect_err("drift conflicts"),
        HistoryError::SubmissionConflict,
        "SI-02d: changed input returns the typed conflict"
    );

    // Adding the allocated explicit Thread to the same key is drift (SI-02b,
    // SI-02f): the original request selected no Thread.
    let selector_drift = identified_command(
        &tenant,
        "subject-a",
        submission,
        Some(accepted.thread_id),
        "first input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &selector_drift,
            std::time::Duration::from_secs(2),
        )
        .expect_err("selector drift conflicts"),
        HistoryError::SubmissionConflict
    );
    assert_eq!(
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission)),
        snapshot,
        "conflicts perform zero mutation"
    );

    // The same UUID under a different subject is an independent key (SI-02a).
    let other_subject = identified_command(&tenant, "subject-b", submission, None, "first input");
    let independent = TurnHistory::accept_initial_with_submission(
        &mut history,
        &other_subject,
        std::time::Duration::from_secs(2),
    )
    .expect("the other subject's key is independent");
    assert!(
        matches!(independent, IdentifiedAcceptance::Created(_)),
        "SI-02a: the same UUID in another subject creates its own binding"
    );
}

/// AC-3/SI-03a/SI-03b/SI-04: four independently connected exact contenders
/// produce exactly one Created owner and three identical observation
/// receipts, one binding, one Turn, one input, and one lease.
#[test]
pub(crate) fn cand_18_concurrent_identity() {
    let harness = Harness::connect(8);
    MIGRATIONS_ONCE.call_once(|| {
        harness.runtime.block_on(async {
            for migration in MIGRATIONS {
                sqlx::raw_sql(migration)
                    .execute(&harness.pool)
                    .await
                    .expect("apply production migration");
            }
        });
    });
    let tenant = format!("cand18-race-{}", Uuid::new_v4());
    let submission = Uuid::new_v4();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let mut handles = Vec::new();
    for contender in 0..4 {
        let harness = &harness;
        let barrier = std::sync::Arc::clone(&barrier);
        let tenant = tenant.clone();
        let executor = harness.executor();
        handles.push(std::thread::spawn(move || {
            let mut history = PostgresTurnHistory::new(executor);
            let command =
                identified_command(&tenant, "subject-a", submission, None, "contended input");
            barrier.wait();
            let outcome = TurnHistory::accept_initial_with_submission(
                &mut history,
                &command,
                std::time::Duration::from_secs(2),
            )
            .expect("every contender resolves to a typed outcome");
            (contender, outcome)
        }));
    }
    let mut created = 0;
    let mut receipts = Vec::new();
    for handle in handles {
        let (_contender, outcome) = handle.join().expect("contender finishes");
        match outcome {
            IdentifiedAcceptance::Created(_) => created += 1,
            IdentifiedAcceptance::Existing(receipt) => receipts.push(receipt),
        }
    }
    assert_eq!(
        created, 1,
        "exactly one contender becomes the created owner (SI-03a)"
    );
    assert_eq!(receipts.len(), 3, "the other contenders observe");
    let first = receipts
        .first()
        .cloned()
        .expect("at least one receipt exists");
    assert!(
        receipts.iter().all(|receipt| receipt == &first),
        "every observer receives the identical receipt (SI-05)"
    );
    let (bindings, turns, inputs, leases) =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(
        (bindings, turns, inputs, leases),
        (1, 1, 1, 1),
        "the contended key commits exactly one canonical acceptance"
    );
}
