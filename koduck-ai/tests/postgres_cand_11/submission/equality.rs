// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 equality-remainder cases: Unicode-composition and whitespace
//! drift are semantic conflicts (SI-02b), a fresh foreign or unknown explicit
//! Thread is the indistinguishable typed `NotFound` (SI-02e), the receipt is
//! constant across every canonical lifecycle status and lease expiry
//! (SI-05/SI-08d), and an exact retry after a post-terminal correction still
//! observes the original acceptance (SI-10).

use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, CorrectionCommand, CorrectionStore, HistoryError, IdentifiedAcceptance, NewItem,
    TurnHistory,
};
use koduck_ai::domain::ThreadId;

use super::super::harness::Harness;
use super::{connected_history, count_rows, identified_command, trust};

/// SI-02b: equality compares the exact UTF-8 bytes with no Unicode
/// normalization and no trimming, so a canonically equivalent but differently
/// encoded retry is the typed conflict with zero mutation.
pub(crate) fn cand_18_normalization_and_whitespace_drift() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();

    // Unicode-composition drift: precomposed "é" vs "e" + combining acute.
    let composition_submission = Uuid::new_v4();
    let composed = identified_command(
        &tenant,
        "subject-a",
        composition_submission,
        None,
        "caf\u{e9} input",
    );
    let IdentifiedAcceptance::Created(_) = TurnHistory::accept_initial_with_submission(
        &mut history,
        &composed,
        Duration::from_secs(2),
    )
    .expect("the composed input creates") else {
        panic!("the composed key must create");
    };
    let decomposed = identified_command(
        &tenant,
        "subject-a",
        composition_submission,
        None,
        "cafe\u{301} input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &decomposed,
            Duration::from_secs(2),
        )
        .expect_err("NFC/NFD drift conflicts"),
        HistoryError::SubmissionConflict,
        "SI-02b: no Unicode normalization is applied"
    );

    // Whitespace drift: the stored bytes keep their padding.
    let whitespace_submission = Uuid::new_v4();
    let padded = identified_command(
        &tenant,
        "subject-a",
        whitespace_submission,
        None,
        "  padded input  ",
    );
    let IdentifiedAcceptance::Created(_) =
        TurnHistory::accept_initial_with_submission(&mut history, &padded, Duration::from_secs(2))
            .expect("the padded input creates")
    else {
        panic!("the padded key must create");
    };
    let trimmed = identified_command(
        &tenant,
        "subject-a",
        whitespace_submission,
        None,
        "padded input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(
            &mut history,
            &trimmed,
            Duration::from_secs(2),
        )
        .expect_err("trimmed drift conflicts"),
        HistoryError::SubmissionConflict,
        "SI-02b: no trimming is applied"
    );

    for submission in [composition_submission, whitespace_submission] {
        let rows =
            harness
                .runtime
                .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
        assert_eq!(rows, (1, 1, 1, 1), "conflicts perform zero mutation");
    }
}

/// SI-02e: a fresh explicit Thread that is unknown, owned by another subject,
/// or owned by another tenant returns the same indistinguishable typed
/// `NotFound` with zero new binding, Thread, or Turn rows.
pub(crate) fn cand_18_foreign_thread_not_found_indistinguishable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let foreign_thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(foreign_thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed the other subject's thread");
    });

    let submissions: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    // Another subject in the same tenant, another tenant entirely, and an
    // unknown Thread id.
    let cases = [
        identified_command(
            &tenant,
            "subject-b",
            submissions[0],
            Some(foreign_thread),
            "foreign subject input",
        ),
        identified_command(
            &format!("{tenant}-other"),
            "subject-a",
            submissions[1],
            Some(foreign_thread),
            "foreign tenant input",
        ),
        identified_command(
            &tenant,
            "subject-a",
            submissions[2],
            Some(ThreadId::new()),
            "unknown thread input",
        ),
    ];
    for case in cases {
        assert_eq!(
            TurnHistory::accept_initial_with_submission(
                &mut history,
                &case,
                Duration::from_secs(2),
            )
            .expect_err("a foreign or unknown thread is not found"),
            HistoryError::NotFound,
            "every rejection is the same indistinguishable typed outcome"
        );
    }
    let bindings: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM chat_submissions WHERE tenant_id = ANY($1)")
            .bind([&tenant, &format!("{tenant}-other")])
            .fetch_one(&harness.pool)
            .await
            .expect("count bindings")
    });
    assert_eq!(bindings, 0, "the rejected keys claim no binding");
    let threads: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM threads WHERE tenant_id = $1")
            .bind(&tenant)
            .fetch_one(&harness.pool)
            .await
            .expect("count threads")
    });
    assert_eq!(threads, 1, "no caller-selected Thread was created");
}

/// SI-05/SI-08d: the receipt is the constant immutable acceptance outcome —
/// every canonical lifecycle status and an expired lease leave the identical
/// receipt with zero mutation.
pub(crate) fn cand_18_receipt_constant_across_lifecycle() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "lifecycle input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the lifecycle binding creates")
    else {
        panic!("the lifecycle key must create");
    };
    let baseline = history
        .submission_observation(&command, Duration::from_secs(2))
        .expect("the started receipt resolves")
        .expect("the started binding is observed");

    for status in [
        "started",
        "recovery-pending",
        "completed",
        "failed",
        "interrupted",
        "cancelled",
    ] {
        harness.runtime.block_on(async {
            sqlx::query("UPDATE turns SET status = $4 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3")
                .bind(&tenant)
                .bind(accepted.thread_id.as_uuid())
                .bind(accepted.turn_id.as_uuid())
                .bind(status)
                .execute(&harness.pool)
                .await
                .expect("set the lifecycle status");
        });
        let observed = history
            .submission_observation(&command, Duration::from_secs(2))
            .expect("the receipt resolves")
            .expect("the binding is observed");
        assert_eq!(
            observed, baseline,
            "status {status} changes no receipt member"
        );
        let retry = TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .expect("the retry resolves");
        assert!(
            matches!(&retry, IdentifiedAcceptance::Existing(receipt) if receipt == &baseline),
            "status {status} keeps the retry an observation: {retry:?}"
        );
    }

    // An expired lease is equally outside the receipt.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 hours', \
             expires_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("expire the lease");
    });
    let observed = history
        .submission_observation(&command, Duration::from_secs(2))
        .expect("the expired-lease receipt resolves")
        .expect("the binding is observed");
    assert_eq!(observed, baseline, "lease expiry changes no receipt member");

    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(rows, (1, 1, 1, 1), "every lifecycle retry mutated nothing");
}

/// SI-10: an exact retry after a post-terminal correction still observes the
/// original acceptance — the equality compares the original raw input, not
/// the corrected effective projection — and performs no new mutation.
pub(crate) fn cand_18_retry_after_correction() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "corrected input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the correction binding creates")
    else {
        panic!("the correction key must create");
    };
    close_and_correct_sequence_one_input(&harness, &mut history, &tenant, &accepted);

    let items_on_turn = |harness: &Harness| {
        harness.runtime.block_on(async {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM turn_items WHERE tenant_id = $1 AND turn_id = $2",
            )
            .bind(&tenant)
            .bind(accepted.turn_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
            .expect("count the turn's items")
        })
    };
    let rows_before = items_on_turn(&harness);
    let observed = history
        .submission_observation(&command, Duration::from_secs(2))
        .expect("the post-correction receipt resolves")
        .expect("the binding is observed");
    assert_eq!(observed.turn_id, accepted.turn_id);
    let rows_after = items_on_turn(&harness);
    assert_eq!(
        rows_before, rows_after,
        "the exact retry after a correction appends nothing"
    );
    let binding_rows = harness.runtime.block_on(async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM chat_submissions WHERE tenant_id = $1 AND submission_id = $2",
        )
        .bind(&tenant)
        .bind(submission)
        .fetch_one(&harness.pool)
        .await
        .expect("count bindings")
    });
    assert_eq!(binding_rows, 1, "the correction leaves one binding");
}

/// Closes the accepted Turn and admits one post-terminal correction of its
/// sequence-1 input; the terminal append took sequence 2, so the correction
/// lands at sequence 3.
fn close_and_correct_sequence_one_input(
    harness: &Harness,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    tenant: &str,
    accepted: &AcceptedTurn,
) {
    TurnHistory::append(
        history,
        accepted,
        NewItem::Terminal(koduck_ai::domain::TerminalOutcome::Cancelled),
    )
    .expect("close the accepted turn");
    let input_item_id: Uuid = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT item_id FROM turn_items WHERE tenant_id = $1 AND thread_id = $2 \
             AND turn_id = $3 AND sequence = 1",
        )
        .bind(tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .fetch_one(&harness.pool)
        .await
        .expect("read the sequence-1 item identity")
    });
    let executor = harness.executor();
    let correction = CorrectionCommand::new(
        trust(tenant, "subject-a"),
        accepted.thread_id,
        accepted.turn_id,
        koduck_ai::domain::ItemId::new(),
        koduck_ai::domain::ItemId::from_uuid(input_item_id),
        "the corrected reading",
    )
    .expect("valid correction command");
    let correction_item = CorrectionStore::correct(&executor, correction)
        .expect("the post-terminal correction admits");
    assert_eq!(correction_item.sequence, 3);
}
