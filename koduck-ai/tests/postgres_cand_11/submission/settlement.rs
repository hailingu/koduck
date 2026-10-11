// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 settlement-remainder cases: each dead-creator proof branch —
//! another invocation's committed creator, or this creator's fenced, expired,
//! or terminal Turn — reconciles to the observation receipt without authority
//! (SI-06c/SI-06d), and a recreated runner handle can never regain execution
//! authority for an existing key (SI-04).

use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, IdentifiedAcceptance, NewItem, TurnHistory, TurnOutcome,
};

use super::super::harness::Harness;
use super::integrity::SubmissionFixture;
use super::runner::{completed_events, scripted_provider};
use super::{connected_history, count_rows, identified_command};

/// SI-06c/SI-06e: a terminal item cannot coexist with a started Turn and
/// authorize its live creator after the write acknowledgement times out.
#[test]
fn cand_18_terminal_item_without_terminal_status_is_unprovable() {
    let _database_guard = super::serialize_database_tests();
    assert_eq!(
        inconsistent_terminal_commit("started", true),
        Err(HistoryError::Unavailable)
    );
}

/// SI-06e: a terminal status without its terminal item is incomplete proof,
/// so even an observation receipt must fail closed after uncertain COMMIT.
#[test]
fn cand_18_terminal_status_without_terminal_item_is_unprovable() {
    let _database_guard = super::serialize_database_tests();
    assert_eq!(
        inconsistent_terminal_commit("cancelled", false),
        Err(HistoryError::Unavailable)
    );
}

/// Drives one isolated deferred COMMIT with inconsistent terminal presence.
/// Teardown precedes the outcome assertion so the intentional red phase
/// cannot leak the fixture schema into later preservation checks.
fn inconsistent_terminal_commit(
    status: &str,
    has_terminal: bool,
) -> Result<IdentifiedAcceptance, HistoryError> {
    let terminal_insert = if has_terminal {
        "INSERT INTO turn_items \
         (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, is_terminal) \
         VALUES (NEW.tenant_id, NEW.thread_id, NEW.turn_id, 2, gen_random_uuid(), \
         'cancelled', '{}', TRUE);"
    } else {
        ""
    };
    let trigger = format!(
        "PERFORM pg_sleep(2.5); {terminal_insert} \
         UPDATE turns SET status = '{status}', next_sequence = {} \
         WHERE tenant_id = NEW.tenant_id AND thread_id = NEW.thread_id \
         AND turn_id = NEW.turn_id;",
        if has_terminal { 3 } else { 2 }
    );
    let fixture = SubmissionFixture::create("inconsistent-terminal", &trigger);
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "terminal integrity proof",
    );
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2));
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    let state = terminal_integrity_state(&fixture.harness, &fixture.pool, &fixture.tenant);
    fixture.teardown();
    assert_eq!(rows, (1, 1, 1, 1), "one durable acceptance remains");
    assert_eq!(state, (status.to_owned(), 1, false, true, has_terminal));
    outcome
}

/// SI-08c: every inconsistent terminal/status pair is rejected by both the
/// unlocked lookup and locked acceptance recheck before drift classification.
#[test]
fn cand_18_terminal_status_inconsistency_fails_lookup_and_recheck() {
    let _database_guard = super::serialize_database_tests();
    for (status, has_terminal) in [
        ("started", true),
        ("recovery-pending", true),
        ("completed", false),
        ("failed", false),
        ("interrupted", false),
        ("cancelled", false),
    ] {
        let (harness, mut history, tenant) = connected_history();
        let submission = Uuid::new_v4();
        let command = identified_command(&tenant, "subject-a", submission, None, "terminal state");
        let IdentifiedAcceptance::Created(accepted) = TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .expect("the fixture's valid acceptance creates") else {
            panic!("the fresh fixture key must create");
        };
        corrupt_terminal_state(&harness, &accepted, status, has_terminal);
        let state_before = terminal_integrity_state(&harness, &harness.pool, &tenant);
        for input in ["terminal state", "drifted terminal state"] {
            let mut candidate = command.clone();
            candidate.input = input.to_owned();
            let lookup = history.submission_observation(&candidate, Duration::from_secs(2));
            let recheck = TurnHistory::accept_initial_with_submission(
                &mut history,
                &candidate,
                Duration::from_secs(2),
            );
            assert_eq!(lookup, Err(HistoryError::Unavailable), "status {status}");
            assert_eq!(recheck, Err(HistoryError::Unavailable), "status {status}");
        }
        assert_eq!(
            terminal_integrity_state(&harness, &harness.pool, &tenant),
            state_before,
            "rejected reads mutate no status, lease, or terminal state"
        );
        let rows =
            harness
                .runtime
                .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
        assert_eq!(rows, (1, 1, 1, 1), "no second acceptance was allocated");
    }
}

/// Seeds one corrupt terminal/status pair atomically in the test tenant.
fn corrupt_terminal_state(harness: &Harness, accepted: &AcceptedTurn, status: &str, present: bool) {
    harness.runtime.block_on(async {
        let mut transaction = harness.pool.begin().await.expect("seed transaction");
        sqlx::query(
            "INSERT INTO turn_items \
             (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, is_terminal) \
             SELECT $1, $2, $3, 2, gen_random_uuid(), 'cancelled', '{}', TRUE WHERE $4",
        )
        .bind(accepted.tenant_id.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(present)
        .execute(&mut *transaction)
        .await
        .expect("seed terminal presence");
        sqlx::query(
            "UPDATE turns SET status = $4, next_sequence = $5 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(accepted.tenant_id.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(status)
        .bind(if present { 3_i64 } else { 2_i64 })
        .execute(&mut *transaction)
        .await
        .expect("seed inconsistent status");
        transaction
            .commit()
            .await
            .expect("commit the corrupt fixture");
    });
}

/// Reads the fixture's status, initial lease, and scoped terminal presence.
fn terminal_integrity_state(
    harness: &Harness,
    pool: &sqlx::PgPool,
    tenant: &str,
) -> (String, i64, bool, bool, bool) {
    harness.runtime.block_on(async {
        sqlx::query_as(
            "SELECT t.status, l.generation, l.fenced, l.expires_at > clock_timestamp(), \
             EXISTS (SELECT 1 FROM turn_items x WHERE x.tenant_id = t.tenant_id \
             AND x.thread_id = t.thread_id AND x.turn_id = t.turn_id AND x.is_terminal) \
             FROM turns t JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
             WHERE t.tenant_id = $1",
        )
        .bind(tenant)
        .fetch_one(pool)
        .await
        .expect("read terminal integrity fixture state")
    })
}

/// SI-06b/SI-07c: a direct caller's 500 ms cannot compress either attempt.
/// A three-second deferred COMMIT exceeds the full write budget but fits the
/// separate proof budget, proving one acceptance without a second write.
#[test]
fn cand_18_direct_proof_preserves_full_budget() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create("full-settlement-budgets", "PERFORM pg_sleep(3);");
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "full settlement budgets",
    );
    let executor = SqlxPostgresExecutor::new(fixture.pool.clone(), fixture.harness.handle());
    let outcome = PostgresExecutor::accept_initial_with_submission(
        &executor,
        &command,
        Duration::from_millis(500),
    );
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    fixture.teardown();
    assert!(
        matches!(outcome, Ok(IdentifiedAcceptance::Created(_))),
        "both reserved attempts retain two seconds: {outcome:?}"
    );
    assert_eq!(
        rows,
        (1, 1, 1, 1),
        "settlement proves one atomic acceptance"
    );
}

/// SI-06c/SI-06e: a live same-invocation lease rewritten to generation 2 inside
/// the delayed COMMIT is unprovable, so settlement cannot publish a receipt or
/// grant Created. The committed canonical acceptance remains exactly once.
#[test]
fn cand_18_live_non_initial_lease_is_unprovable() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create(
        "live-non-initial",
        "PERFORM pg_sleep(2.5); \
         UPDATE turn_leases SET generation = 2 WHERE tenant_id = NEW.tenant_id \
         AND thread_id = NEW.thread_id AND turn_id = NEW.turn_id;",
    );
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "lease proof",
    );
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2));
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    let lease: (i64, bool, bool, String) = fixture.harness.runtime.block_on(async {
        sqlx::query_as(
            "SELECT l.generation, l.fenced, l.expires_at > CURRENT_TIMESTAMP, t.status \
             FROM turn_leases l JOIN turns t USING (tenant_id, thread_id, turn_id) \
             WHERE l.tenant_id = $1",
        )
        .bind(&fixture.tenant)
        .fetch_one(&fixture.pool)
        .await
        .expect("read the rewritten live lease")
    });
    fixture.teardown();
    assert_eq!(outcome, Err(HistoryError::Unavailable));
    assert_eq!(
        rows,
        (1, 1, 1, 0),
        "one acceptance with no generation-1 lease"
    );
    assert_eq!(lease, (2, false, true, "started".to_owned()));
}

/// AC-5/SI-06c/SI-06d (ADR-0018): when this invocation's write acknowledgement
/// is lost and the delayed commit lands the binding in a dead-creator state,
/// the read-only proof returns the observation receipt — never `Created` and
/// never unavailable — for a different committed creator, and for this
/// creator's fenced, expired, and terminal Turn. Each variant still commits
/// exactly one durable acceptance.
pub(crate) fn cand_18_dead_creator_settlement_branches() {
    let _database_guard = super::serialize_database_tests();
    let branches = [
        (
            "creator-mismatch",
            "UPDATE chat_submissions SET creator_operation_id = gen_random_uuid() \
             WHERE submission_id = NEW.submission_id;",
        ),
        (
            "fenced-creator",
            "UPDATE turn_leases SET fenced = TRUE WHERE tenant_id = NEW.tenant_id \
             AND thread_id = NEW.thread_id AND turn_id = NEW.turn_id;",
        ),
        (
            "expired-creator",
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 hours', \
             expires_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' \
             WHERE tenant_id = NEW.tenant_id AND thread_id = NEW.thread_id \
             AND turn_id = NEW.turn_id;",
        ),
        (
            "terminal-creator",
            "INSERT INTO turn_items \
             (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, is_terminal) \
             VALUES (NEW.tenant_id, NEW.thread_id, NEW.turn_id, 2, gen_random_uuid(), \
             'completed', '{\"input_tokens\":0,\"output_tokens\":0,\"total_tokens\":0}', TRUE); \
             UPDATE turns SET status = 'completed', next_sequence = 3 \
             WHERE tenant_id = NEW.tenant_id AND thread_id = NEW.thread_id \
             AND turn_id = NEW.turn_id;",
        ),
    ];
    for (label, rewrite) in branches {
        let trigger = format!("PERFORM pg_sleep(2.5); {rewrite}");
        let fixture = SubmissionFixture::create(label, &trigger);
        let mut history = fixture.history();
        let submission = Uuid::new_v4();
        let command = identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            None,
            "dead creator input",
        );
        let outcome = TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .unwrap_or_else(|error| panic!("branch {label} resolves, got {error:?}"));
        let IdentifiedAcceptance::Existing(receipt) = &outcome else {
            panic!("branch {label} observes without authority, got {outcome:?}");
        };
        assert_eq!(
            receipt.submission_id.as_uuid(),
            submission,
            "branch {label} publishes only the client's identity"
        );
        let (bindings, turns, inputs, unfenced_leases) = fixture.harness.runtime.block_on(
            count_rows(&fixture.pool, &fixture.tenant, "subject-a", submission),
        );
        // The fenced branch rewrites its lease row in place, so its unfenced
        // generation-1 lease count is zero while the acceptance stays intact.
        let expected_leases = i64::from(label != "fenced-creator");
        assert_eq!(
            (bindings, turns, inputs, unfenced_leases),
            (1, 1, 1, expected_leases),
            "branch {label} still commits exactly one acceptance"
        );
        fixture.teardown();
    }
}

/// AC-5/SI-04 (ADR-0018): process restart cannot turn an existing key into a
/// new execution grant. A recreated runner handle — fresh provider, fresh
/// clock, same durable state — observes an in-flight `started` Turn without
/// renewing its lease or invoking the provider, and observes a terminal Turn
/// with the identical receipt.
pub(crate) fn cand_18_restart_cannot_regain_authority() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    restart_observes_started_turn(&harness, &mut history, &tenant);
    restart_observes_terminal_turn(&harness, &mut history, &tenant);
}

/// The recreated handle observes an orphaned `started` Turn with no lease
/// renewal and no provider invocation (SI-04).
fn restart_observes_started_turn(
    harness: &Harness,
    history: &mut PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    tenant: &str,
) {
    let submission = Uuid::new_v4();
    let command = identified_command(tenant, "subject-a", submission, None, "orphan input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(history, &command, Duration::from_secs(2))
            .expect("the orphan acceptance creates")
    else {
        panic!("the orphan key must create");
    };
    let (provider, observed) = scripted_provider(completed_events());
    let mut restarted = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let outcome = restarted
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the recreated handle resolves the started key");
    let TurnOutcome::Observed(receipt) = &outcome else {
        panic!("a recreated runner observes a started Turn: {outcome:?}");
    };
    assert_eq!(receipt.turn_id, accepted.turn_id);
    assert!(
        observed.lock().expect("inputs").is_empty(),
        "the restarted handle invokes no provider"
    );
    let generation: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT generation FROM turn_leases WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .fetch_one(&harness.pool)
        .await
        .expect("read the orphan lease generation")
    });
    assert_eq!(generation, 1, "an observer renews no lease");
}

/// The recreated handle observes a completed Turn with the identical receipt
/// and no provider invocation (SI-04).
fn restart_observes_terminal_turn(
    harness: &Harness,
    history: &mut PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    tenant: &str,
) {
    let submission = Uuid::new_v4();
    let command = identified_command(tenant, "subject-a", submission, None, "terminal input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(history, &command, Duration::from_secs(2))
            .expect("the terminal acceptance creates")
    else {
        panic!("the terminal key must create");
    };
    TurnHistory::append(
        history,
        &accepted,
        NewItem::Terminal(koduck_ai::domain::TerminalOutcome::Completed {
            usage: koduck_ai::domain::Usage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            },
        }),
    )
    .expect("close the terminal turn");
    let (provider, observed) = scripted_provider(completed_events());
    let mut second_restart = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let outcome = second_restart
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the recreated handle resolves the terminal key");
    let TurnOutcome::Observed(receipt) = &outcome else {
        panic!("a recreated runner observes a terminal Turn: {outcome:?}");
    };
    assert_eq!(receipt.turn_id, accepted.turn_id);
    assert!(
        observed.lock().expect("inputs").is_empty(),
        "the terminal observer invokes no provider"
    );
    let status: String = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT status FROM turns WHERE tenant_id = $1 AND turn_id = $2")
            .bind(tenant)
            .bind(accepted.turn_id.as_uuid())
            .fetch_one(&harness.pool)
            .await
            .expect("read the terminal status")
    });
    assert_eq!(status, "completed");
}
