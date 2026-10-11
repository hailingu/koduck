// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 acceptance-domain cases against the real migrated `PostgreSQL`:
//! one atomic binding, exact equality and owner scope, contended creator
//! selection, and the explicit-selector existence rule.

use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{PostgresExecutor, PostgresTurnHistory};
use koduck_ai::application::{HistoryError, IdentifiedAcceptance, TurnCommand, TurnHistory};
use koduck_ai::domain::ThreadId;

use super::super::harness::{Harness, MIGRATIONS, MIGRATIONS_ONCE};
use super::{connected_history, count_rows, identified_command, trust};

/// SI-01d: the public history's legacy acceptance must reject a supplied key
/// on every retry instead of creating unbound canonical state.
#[test]
fn cand_18_legacy_history_rejects_identified() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let command = identified_command(&tenant, "subject-a", Uuid::new_v4(), None, "legacy entry");
    for _ in 0..2 {
        assert_legacy_acceptance_rejected(
            &harness,
            &tenant,
            &TurnHistory::accept_initial(&mut history, &command),
        );
    }
}

/// SI-01d: calling the production executor directly cannot bypass the legacy
/// entry's submission guard or create unbound canonical state on a retry.
#[test]
fn cand_18_legacy_executor_rejects_identified() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let command = identified_command(&tenant, "subject-a", Uuid::new_v4(), None, "legacy entry");
    for _ in 0..2 {
        assert_legacy_acceptance_rejected(
            &harness,
            &tenant,
            &PostgresExecutor::accept_initial(&harness.executor(), &command),
        );
    }
}

/// Checks the typed rejection and every canonical table independently of a
/// binding, so an incorrectly accepted unbound Turn cannot hide from the test.
fn assert_legacy_acceptance_rejected(
    harness: &Harness,
    tenant: &str,
    result: &Result<koduck_ai::application::AcceptedTurn, HistoryError>,
) {
    assert_eq!(result, &Err(HistoryError::Unavailable));
    let rows: (i64, i64, i64, i64, i64) = harness.runtime.block_on(async {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM threads WHERE tenant_id = $1), \
             (SELECT count(*) FROM turns WHERE tenant_id = $1), \
             (SELECT count(*) FROM turn_items WHERE tenant_id = $1), \
             (SELECT count(*) FROM turn_leases WHERE tenant_id = $1), \
             (SELECT count(*) FROM chat_submissions WHERE tenant_id = $1)",
        )
        .bind(tenant)
        .fetch_one(&harness.pool)
        .await
        .expect("count all canonical rows")
    });
    assert_eq!(rows, (0, 0, 0, 0, 0), "rejection leaves no durable rows");
}

/// SI-01d: both legacy entries reject before I/O, even when the canonical
/// relation is exclusively locked and an attempted write would time out.
#[test]
fn cand_18_legacy_acceptance_guard_precedes_io() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let command = identified_command(&tenant, "subject-a", Uuid::new_v4(), None, "locked entry");
    let mut lock = harness
        .runtime
        .block_on(harness.pool.begin())
        .expect("lock transaction");
    harness.runtime.block_on(async {
        sqlx::query("LOCK TABLE threads IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .expect("exclude canonical I/O");
    });
    let results = [false, true].map(|executor_entry| {
        let started = std::time::Instant::now();
        let result = if executor_entry {
            PostgresExecutor::accept_initial(&harness.executor(), &command)
        } else {
            TurnHistory::accept_initial(&mut history, &command)
        };
        (result, started.elapsed())
    });
    harness
        .runtime
        .block_on(lock.rollback())
        .expect("release table lock");
    for (result, elapsed) in results {
        assert_eq!(result, Err(HistoryError::Unavailable));
        assert!(
            elapsed < Duration::from_secs(1),
            "reject before database wait"
        );
    }
    assert_legacy_acceptance_rejected(&harness, &tenant, &Err(HistoryError::Unavailable));
}

pub(crate) fn cand_18_atomic_acceptance() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "first input");
    let created =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
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
    let retry =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
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
pub(crate) fn cand_18_equality_and_scope() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "first input");
    let created =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
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
            Duration::from_secs(2),
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
            Duration::from_secs(2),
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
        Duration::from_secs(2),
    )
    .expect("the other subject's key is independent");
    assert!(
        matches!(independent, IdentifiedAcceptance::Created(_)),
        "SI-02a: the same UUID in another subject creates its own binding"
    );
}

/// AC-3/SI-03a/SI-03b/SI-04: four independently connected exact contenders
/// produce exactly one Created owner and three identical observation
pub(crate) fn cand_18_concurrent_identity() {
    let _database_guard = super::serialize_database_tests();
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
                Duration::from_secs(2),
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

/// A deterministic provider that replays one scripted event sequence and then
#[test]
fn cand_18_explicit_thread_must_exist() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();

    // Unknown explicit Thread: the typed rejection without any write.
    let unknown_thread = ThreadId::new();
    let command = identified_command(
        &tenant,
        "subject-a",
        submission,
        Some(unknown_thread),
        "explicit input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("an unknown explicit thread is rejected"),
        HistoryError::NotFound
    );
    let created_thread: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM threads WHERE thread_id = $1")
            .bind(unknown_thread.as_uuid())
            .fetch_one(&harness.pool)
            .await
            .expect("count the unknown thread row")
    });
    assert_eq!(
        created_thread, 0,
        "the caller-selected Thread must not be created by the identified path"
    );
    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(rows, (0, 0, 0, 0), "the rejected key claims no binding");

    // Existing owned explicit Thread: the acceptance proceeds.
    let owned_thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(owned_thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed the owned thread");
    });
    let command = identified_command(
        &tenant,
        "subject-a",
        submission,
        Some(owned_thread),
        "explicit input",
    );
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the existing owned explicit thread is accepted");
    let IdentifiedAcceptance::Created(accepted) = &outcome else {
        panic!("the owned explicit thread must create, got {outcome:?}");
    };
    assert_eq!(accepted.thread_id, owned_thread);
}

/// SI-03a (review round 7, finding 2): the owned explicit Thread row stays
/// locked through final acceptance — a concurrent writer holding the row
/// blocks the acceptance until its budget expires instead of letting the
/// upsert recreate the Thread beneath the unlocked precheck.
#[test]
fn cand_18_explicit_thread_row_locks_through_acceptance() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed the owned thread");
    });

    // A concurrent writer holds the Thread row until released.
    let lock = harness.runtime.block_on(async {
        let mut guard = harness.pool.begin().await.expect("lock transaction starts");
        sqlx::query("SELECT 1 FROM threads WHERE tenant_id = $1 AND thread_id = $2 FOR UPDATE")
            .bind(&tenant)
            .bind(thread.as_uuid())
            .fetch_one(&mut *guard)
            .await
            .expect("hold the thread row");
        guard
    });

    let submission = Uuid::new_v4();
    let command = identified_command(
        &tenant,
        "subject-a",
        submission,
        Some(thread),
        "locked input",
    );
    let started = std::time::Instant::now();
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the locked owned row blocks the acceptance"),
        HistoryError::Unavailable
    );
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "the acceptance waited on the row lock for its full write budget"
    );

    // Releasing the lock lets the same key accept on the existing row.
    harness.runtime.block_on(async move {
        drop(lock);
    });
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the released row accepts");
    assert!(matches!(outcome, IdentifiedAcceptance::Created(_)));
}

/// SI-03a (review round 21): the initial lease is timestamped with the
/// statement clock after the submission-lock wait, so a contended winning
/// request commits with the full authoritative lease window ahead of it
/// instead of one already shortened by its own lock wait.
#[test]
fn cand_18_contended_lease_window_is_full() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let trust_context = trust(&tenant, "subject-a");
    let keys = koduck_ai::application::submission_lock_keys(
        &trust_context,
        koduck_ai::domain::SubmissionId::from_uuid(submission).expect("non-nil submission"),
    );

    // Hold the submission lock while the contender's transaction has already
    // begun, so a transaction-start timestamp predates the lock wait.
    let lock_pool = harness.pool.clone();
    let mut holder = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("holder transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(keys.0)
            .bind(keys.1)
            .execute(&mut *holder)
            .await
            .expect("hold the submission lock");
    });
    let executor = harness.executor();
    let contender_tenant = tenant.clone();
    let handle = std::thread::spawn(move || {
        let mut contender = PostgresTurnHistory::new(executor);
        let command = identified_command(
            &contender_tenant,
            "subject-a",
            submission,
            None,
            "contended lease input",
        );
        TurnHistory::accept_initial_with_submission(
            &mut contender,
            &command,
            Duration::from_secs(2),
        )
        .expect("the contender accepts once the lock releases")
    });
    std::thread::sleep(Duration::from_millis(1_500));
    harness.runtime.block_on(async move {
        drop(holder);
    });
    let outcome = handle.join().expect("the contender finishes");
    assert!(
        matches!(outcome, IdentifiedAcceptance::Created(_)),
        "the released lock lets the contender create: {outcome:?}"
    );

    // The remaining lease window at observation time keeps (nearly) the full
    // twenty seconds: the lease was renewed at statement time, after the
    // wait, not at the transaction's frozen start.
    let remaining_ms: f64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT (EXTRACT(EPOCH FROM (l.expires_at - clock_timestamp())) \
             * 1000.0)::FLOAT8 \
             FROM turn_leases l JOIN chat_submissions s \
             ON s.tenant_id = l.tenant_id AND s.thread_id = l.thread_id \
             AND s.turn_id = l.turn_id \
             WHERE s.tenant_id = $1 AND s.submission_id = $2 AND l.generation = 1",
        )
        .bind(&tenant)
        .bind(submission)
        .fetch_one(&harness.pool)
        .await
        .expect("read the contended lease window")
    });
    assert!(
        remaining_ms > 19_000.0,
        "the contended lease keeps the full window, {remaining_ms} ms remain"
    );
}
