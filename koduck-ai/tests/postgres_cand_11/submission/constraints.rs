// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 binding-integrity-remainder cases: each migration constraint
//! independently rejects its invalid row (SI-08b), the binding survives the
//! retained-canonical lifetime transitions with the identical receipt
//! (SI-08d), and the foreign key prevents a binding from outliving its Turn
//! while a rolled-back canonical deletion leaves the structure intact.

use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, IdentifiedAcceptance, NewItem, SubmissionObservation, TurnCommand,
    TurnHistory,
};
use koduck_ai::domain::{ThreadId, TurnId};

use super::super::harness::Harness;
use super::{connected_history, count_rows, identified_command};

/// SI-08b: the scoped primary key, accepted-Turn uniqueness, Turn foreign key,
/// explicit-selector equality, and the non-nil submission and creator checks
/// each reject an individually invalid row with their named constraint.
pub(crate) fn cand_18_binding_constraints_reject_invalid_rows() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "constraint input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the constraint fixture acceptance creates")
    else {
        panic!("the constraint key must create");
    };
    for (expected_constraint, statement) in
        constraint_violation_cases(&tenant, submission, accepted.thread_id, accepted.turn_id)
    {
        let error = harness
            .runtime
            .block_on(async {
                sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
                    .execute(&harness.pool)
                    .await
            })
            .expect_err("the constraint rejects the invalid row");
        let constraint = error
            .as_database_error()
            .and_then(|database| database.constraint())
            .unwrap_or_else(|| panic!("constraint {expected_constraint} reports its name"));
        assert_eq!(
            constraint, expected_constraint,
            "the invalid row is rejected by its named constraint"
        );
    }
    // The valid fixture binding itself is untouched by the rejected rows.
    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(rows, (1, 1, 1, 1));
}

/// One individually invalid `chat_submissions` row per SI-08b constraint: the
/// duplicate scoped key, the duplicate accepted Turn, the orphan turn
/// reference, the mismatched explicit selector, and the nil submission and
/// creator identities.
fn constraint_violation_cases(
    tenant: &str,
    submission: Uuid,
    thread: ThreadId,
    turn: TurnId,
) -> Vec<(&'static str, String)> {
    let thread = thread.as_uuid();
    let turn = turn.as_uuid();
    let other_thread = Uuid::new_v4();
    let other_turn = Uuid::new_v4();
    vec![
        (
            "chat_submissions_pkey",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{submission}', '{other_thread}', \
                 '{other_turn}', '{}')",
                Uuid::new_v4()
            ),
        ),
        (
            "chat_submissions_turn_unique",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{}', '{thread}', '{turn}', '{}')",
                Uuid::new_v4(),
                Uuid::new_v4()
            ),
        ),
        (
            "chat_submissions_turn_fk",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{}', '{other_thread}', \
                 '{other_turn}', '{}')",
                Uuid::new_v4(),
                Uuid::new_v4()
            ),
        ),
        (
            "chat_submissions_original_selector_check",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, original_thread_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{}', '{thread}', '{turn}', \
                 '{other_thread}', '{}')",
                Uuid::new_v4(),
                Uuid::new_v4()
            ),
        ),
        (
            "chat_submissions_non_nil_submission_id",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{}', '{other_thread}', \
                 '{other_turn}', '{}')",
                Uuid::nil(),
                Uuid::new_v4()
            ),
        ),
        (
            "chat_submissions_non_nil_creator_operation",
            format!(
                "INSERT INTO chat_submissions (tenant_id, subject_id, submission_id, \
                 thread_id, turn_id, creator_operation_id) \
                 VALUES ('{tenant}', 'subject-a', '{}', '{other_thread}', \
                 '{other_turn}', '{}')",
                Uuid::new_v4(),
                Uuid::nil()
            ),
        ),
    ]
}

/// SI-08d: the binding survives the retained-canonical lifetime transitions —
/// terminal closure, lease expiry, and a fresh acceptance handle acting as a
/// process restart — always returning the identical receipt with no new
/// canonical work, while the foreign key keeps the binding from outliving
/// its Turn and a rolled-back canonical deletion leaves it provable.
pub(crate) fn cand_18_binding_lifetime_preserved_across_transitions() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "lifetime input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the lifetime acceptance creates")
    else {
        panic!("the lifetime key must create");
    };
    let baseline = lifetime_receipts_stay_constant(&harness, &mut history, &command, &accepted);
    foreign_key_and_rollback_preserve_structure(&harness, &mut history, &command, &accepted);

    // The retry through the acceptance port stays an observation, and drift
    // keeps its typed conflict across the same transitions.
    assert!(matches!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .expect("the lifetime retry resolves"),
        IdentifiedAcceptance::Existing(receipt) if receipt == baseline
    ));
    let drifted = identified_command(
        &tenant,
        "subject-a",
        submission,
        None,
        "changed lifetime input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &drifted,
            Duration::from_secs(2),
        )
        .expect_err("lifetime drift conflicts"),
        HistoryError::SubmissionConflict
    );
}

/// Terminal closure, lease expiry, and a fresh acceptance handle acting as a
/// process restart each leave the identical receipt (SI-08d). Returns the
/// baseline receipt for the caller's final assertions.
fn lifetime_receipts_stay_constant(
    harness: &Harness,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    command: &TurnCommand,
    accepted: &AcceptedTurn,
) -> SubmissionObservation {
    let tenant = command.trust.tenant_id.as_str();
    let baseline = history
        .submission_observation(command, Duration::from_secs(2))
        .expect("the baseline receipt resolves")
        .expect("the lifetime binding is observed");

    // Terminal closure: the binding survives with the identical receipt.
    TurnHistory::append(
        history,
        accepted,
        NewItem::Terminal(koduck_ai::domain::TerminalOutcome::Cancelled),
    )
    .expect("close the lifetime turn");
    assert_eq!(
        history
            .submission_observation(command, Duration::from_secs(2))
            .expect("the terminal receipt resolves")
            .expect("the binding survives the terminal"),
        baseline
    );

    // Lease expiry: the binding survives unchanged.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 hours', \
             expires_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' \
             WHERE tenant_id = $1 AND turn_id = $2",
        )
        .bind(tenant)
        .bind(accepted.turn_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("expire the lifetime lease");
    });
    assert_eq!(
        history
            .submission_observation(command, Duration::from_secs(2))
            .expect("the expired receipt resolves")
            .expect("the binding survives the expiry"),
        baseline
    );

    // Process restart: a fresh acceptance handle still observes the same key.
    let restarted = PostgresTurnHistory::new(harness.executor());
    assert_eq!(
        restarted
            .submission_observation(command, Duration::from_secs(2))
            .expect("the restarted receipt resolves")
            .expect("the binding survives the restart"),
        baseline
    );
    baseline
}

/// The foreign key prevents the canonical Turn from being deleted beneath the
/// binding, and a canonical deletion attempted inside a transaction that
/// rolls back leaves the sequence-1 input and the provable binding intact.
fn foreign_key_and_rollback_preserve_structure(
    harness: &Harness,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    command: &TurnCommand,
    accepted: &AcceptedTurn,
) {
    let baseline = history
        .submission_observation(command, Duration::from_secs(2))
        .expect("the pre-deletion receipt resolves")
        .expect("the lifetime binding is observed");
    blocked_turn_deletion_rolls_back(harness, history, command, accepted, &baseline);
    rolled_back_input_deletion_stays_invisible(harness, history, command, accepted, &baseline);
}

/// Clearing the Turn's items and leases first leaves the binding's foreign
/// key as the sole named deletion blocker, and aborting the transaction rolls
/// every deletion back so the provable binding survives intact.
fn blocked_turn_deletion_rolls_back(
    harness: &Harness,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    command: &TurnCommand,
    accepted: &AcceptedTurn,
    baseline: &SubmissionObservation,
) {
    let tenant = command.trust.tenant_id.as_str();
    let delete_pool = harness.pool.clone();
    let mut deletion = harness
        .runtime
        .block_on(delete_pool.begin())
        .expect("deletion transaction starts");
    for statement in [
        "DELETE FROM turn_items WHERE tenant_id = $1 AND turn_id = $2",
        "DELETE FROM turn_leases WHERE tenant_id = $1 AND turn_id = $2",
        "DELETE FROM turns WHERE tenant_id = $1 AND turn_id = $2",
    ] {
        let outcome = harness.runtime.block_on(async {
            sqlx::query(statement)
                .bind(tenant)
                .bind(accepted.turn_id.as_uuid())
                .execute(&mut *deletion)
                .await
        });
        if statement.starts_with("DELETE FROM turns") {
            assert!(
                outcome
                    .expect_err("the foreign key protects the canonical turn")
                    .as_database_error()
                    .and_then(|database| database.constraint())
                    .is_some_and(|constraint| constraint == "chat_submissions_turn_fk"),
                "the blocked deletion names the binding foreign key"
            );
        }
    }
    harness.runtime.block_on(async move {
        drop(deletion);
    });
    assert_eq!(
        history
            .submission_observation(command, Duration::from_secs(2))
            .expect("the rolled-back deletion resolves")
            .expect("the binding and its canonical rows survive"),
        baseline.clone(),
        "the aborted deletion leaves the structure intact"
    );
}

/// A canonical input deletion attempted inside a transaction that rolls back
/// stays invisible to the unlocked committed read and restores the provable
/// binding afterwards.
fn rolled_back_input_deletion_stays_invisible(
    harness: &Harness,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    command: &TurnCommand,
    accepted: &AcceptedTurn,
    baseline: &SubmissionObservation,
) {
    let tenant = command.trust.tenant_id.as_str();
    let rollback_pool = harness.pool.clone();
    let mut rollback = harness
        .runtime
        .block_on(rollback_pool.begin())
        .expect("rollback transaction starts");
    harness.runtime.block_on(async {
        sqlx::query(
            "DELETE FROM turn_items WHERE tenant_id = $1 AND turn_id = $2 AND sequence = 1",
        )
        .bind(tenant)
        .bind(accepted.turn_id.as_uuid())
        .execute(&mut *rollback)
        .await
        .expect("the in-transaction deletion runs");
    });
    assert_eq!(
        history
            .submission_observation(command, Duration::from_secs(2))
            .expect("the mid-rollback receipt resolves")
            .expect("uncommitted deletion is invisible"),
        baseline.clone()
    );
    harness.runtime.block_on(async move {
        drop(rollback);
    });
    assert_eq!(
        history
            .submission_observation(command, Duration::from_secs(2))
            .expect("the post-rollback receipt resolves")
            .expect("the rolled-back deletion restores the input"),
        baseline.clone(),
        "the rolled-back deletion preserves FK integrity"
    );
}
