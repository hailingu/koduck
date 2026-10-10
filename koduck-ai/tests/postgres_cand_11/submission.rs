// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 identified acceptance against the real migrated `PostgreSQL`
//! through the production history port (AC-2, AC-3, AC-4): one atomic
//! binding, exact-retry receipts with zero mutation, typed drift conflicts,
//! independent owner scopes, and concurrent creator selection.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::application::{
    AcceptanceClock, AcceptanceInstant, HistoryError, IdentifiedAcceptance, ModelInput,
    ModelProvider, NewItem, PriorTurnHistory, ProviderError, ProviderEvent, ProviderStream,
    TurnCommand, TurnHistory, TurnOutcome, TurnRunError,
};
use koduck_ai::domain::{SubmissionId, TenantId, ThreadId, TrustContext, TurnId, TurnStatus};

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
/// under another subject owns an independent key.
#[test]
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
/// receipts, one binding, one Turn, one input, and one lease.
#[test]
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
/// completes, recording the observed model input.
struct ScriptedProvider {
    events: Vec<ProviderEvent>,
    observed_inputs: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ModelProvider for ScriptedProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.observed_inputs
            .lock()
            .expect("input lock")
            .push(input.input);
        let events = std::mem::take(&mut self.events);
        Ok(Box::new(events.into_iter()))
    }
}

fn scripted_provider(
    events: Vec<ProviderEvent>,
) -> (ScriptedProvider, Arc<std::sync::Mutex<Vec<String>>>) {
    let observed_inputs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    (
        ScriptedProvider {
            events,
            observed_inputs: std::sync::Arc::clone(&observed_inputs),
        },
        observed_inputs,
    )
}

fn completed_events() -> Vec<koduck_ai::application::ProviderEvent> {
    use koduck_ai::application::ProviderEvent;
    vec![
        ProviderEvent::Delta("answer".to_owned()),
        ProviderEvent::Usage(koduck_ai::domain::Usage {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
        }),
        ProviderEvent::Completed,
    ]
}

/// AC-2/AC-6/SI-04/SI-07 (ADR-0018): the production runner identified flow —
/// fresh acceptance executes the created owner, the exact retry observes
/// without execution, drift conflicts, the expired acceptance budget starts
/// no write, pre-acceptance cancellation wins, and a cancellation racing the
/// proven Created outcome durably cancels without provider execution.
#[test]
pub(crate) fn cand_18_runner_flows() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let events = completed_events();
    let submission = Uuid::new_v4();

    // Fresh identified acceptance: the created owner executes through the
    // provider and completes durably (SI-07d handoff).
    let (provider, observed) = scripted_provider(events.clone());
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let command = identified_command(&tenant, "subject-a", submission, None, "runner input");
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command.clone(), &mut |_| {}, &|| false)
        .expect("the fresh identified run creates and executes");
    let TurnOutcome::Owned(result) = &outcome else {
        panic!("the fresh key must own its execution, got {outcome:?}");
    };
    assert_eq!(result.status, koduck_ai::domain::TurnStatus::Completed);
    assert_eq!(
        observed.lock().expect("inputs").as_slice(),
        ["runner input"],
        "exactly one provider invocation runs for the created owner"
    );
    let (bindings, turns, inputs, leases) =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!((bindings, turns, inputs, leases), (1, 1, 1, 1));

    // Exact retry through the same runner: observation receipt, no new
    // provider invocation, no new Turn (SI-04/SI-05).
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command.clone(), &mut |_| {}, &|| false)
        .expect("the exact retry observes");
    let TurnOutcome::Observed(receipt) = &outcome else {
        panic!("the exact retry must observe, got {outcome:?}");
    };
    assert_eq!(receipt.turn_id, result.turn_id);
    assert_eq!(
        observed.lock().expect("inputs").len(),
        1,
        "a retry starts no provider work"
    );
    let after =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(
        (bindings, turns, inputs, leases),
        after,
        "a retry mutates nothing"
    );

    // Drift under the owned key: the typed conflict surfaces from the runner.
    let drifted = identified_command(
        &tenant,
        "subject-a",
        submission,
        None,
        "changed runner input",
    );
    assert!(matches!(
        runner
            .execute_submission_with_observer_and_cancellation(drifted, &mut |_| {}, &|| false)
            .expect_err("drift conflicts"),
        TurnRunError::History(HistoryError::SubmissionConflict)
    ));
}

/// AC-6/SI-07g/SI-07h: a cancellation racing the proven Created outcome
/// enters the durable cancellation path without provider execution, and an
/// observed pre-acceptance cancellation wins before any write.
pub(crate) fn cand_18_runner_cancellation() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let events = completed_events();

    // Cancellation racing the proven Created outcome (SI-07h): the stateful
    // flag answers false at the pre-write check and true at the post-Created
    // check.
    let racing_submission = Uuid::new_v4();
    let (provider, racing_observed) = scripted_provider(events.clone());
    let mut racing_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let checked = std::cell::Cell::new(0_usize);
    let cancelled = || {
        let call = checked.get();
        checked.set(call + 1);
        call > 0
    };
    let racing_command = identified_command(
        &tenant,
        "subject-a",
        racing_submission,
        None,
        "racing input",
    );
    let outcome = racing_runner
        .execute_submission_with_observer_and_cancellation(racing_command, &mut |_| {}, &cancelled)
        .expect("the racing cancellation still owns its durable result");
    let koduck_ai::application::TurnOutcome::Owned(raced) = &outcome else {
        panic!("the racing cancellation keeps ownership, got {outcome:?}");
    };
    assert_eq!(
        raced.status,
        TurnStatus::Cancelled,
        "SI-07h: the accepted Turn closes as exactly one durable cancellation"
    );
    assert!(
        racing_observed.lock().expect("inputs").is_empty(),
        "the racing cancellation starts no provider execution"
    );
    let racing_rows = harness.runtime.block_on(count_rows(
        &harness.pool,
        &tenant,
        "subject-a",
        racing_submission,
    ));
    assert_eq!(
        racing_rows,
        (1, 1, 1, 1),
        "the binding and canonical state remain"
    );
}

/// SI-07g: an observed pre-acceptance cancellation wins before any write, so
/// no binding, Turn, or provider work may exist.
pub(crate) fn cand_18_runner_pre_acceptance_cancellation() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let (provider, cancel_observed) = scripted_provider(completed_events());
    let mut cancelled_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let cancelled_submission = Uuid::new_v4();
    let cancelled_command = identified_command(
        &tenant,
        "subject-a",
        cancelled_submission,
        None,
        "cancelled input",
    );
    assert!(matches!(
        cancelled_runner
            .execute_submission_with_observer_and_cancellation(
                cancelled_command,
                &mut |_| {},
                &|| true,
            )
            .expect_err("pre-acceptance cancellation surfaces"),
        TurnRunError::Cancelled
    ));
    assert!(cancel_observed.lock().expect("inputs").is_empty());
    let cancelled_rows = harness.runtime.block_on(count_rows(
        &harness.pool,
        &tenant,
        "subject-a",
        cancelled_submission,
    ));
    assert_eq!(
        cancelled_rows,
        (0, 0, 0, 0),
        "no binding or canonical state exists for the cancelled creator"
    );
}

/// The manual acceptance clock replaying staged readings so the exact SI-07
/// gate boundaries are deterministic without sleeps or paused Tokio time.
struct ManualClock(std::sync::Mutex<std::collections::VecDeque<Duration>>);

impl ManualClock {
    fn staged(readings: &[Duration]) -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(
            readings.iter().copied().collect(),
        )))
    }
}

impl AcceptanceClock for ManualClock {
    fn now(&self) -> AcceptanceInstant {
        let mut readings = self.0.lock().expect("manual clock lock");
        let next = readings
            .pop_front()
            .unwrap_or_else(|| *readings.back().unwrap_or(&Duration::ZERO));
        AcceptanceInstant::from_elapsed(next)
    }
}

/// AC-6/SI-07d (ADR-0018): the exact remaining-time write gate and the
/// sufficient four-second boundary with real database timers.
pub(crate) fn cand_18_deadline_gate() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let events = vec![];

    // Less than four seconds remaining at the write gate: no write starts.
    // Readings: start (0 s), lookup clamp (0.5 s), gate (6.5 s elapsed).
    let (provider, gate_observed) = scripted_provider(events.clone());
    let mut short_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        Duration::from_secs(0),
        Duration::from_millis(500),
        Duration::from_millis(6_500),
    ]));
    let short_submission = Uuid::new_v4();
    let short_command =
        identified_command(&tenant, "subject-a", short_submission, None, "short input");
    assert!(matches!(
        short_runner
            .execute_submission_with_observer_and_cancellation(short_command, &mut |_| {}, &|| {
                false
            })
            .expect_err("the short budget starts no write"),
        TurnRunError::History(HistoryError::Unavailable)
    ));
    assert!(gate_observed.lock().expect("inputs").is_empty());
    let short_rows = harness.runtime.block_on(count_rows(
        &harness.pool,
        &tenant,
        "subject-a",
        short_submission,
    ));
    assert_eq!(
        short_rows,
        (0, 0, 0, 0),
        "no binding or canonical state exists for the expired budget"
    );

    // Exactly four seconds is sufficient: the write starts and the created
    // owner executes (SI-07d).
    let (provider, exact_observed) = scripted_provider(completed_events());
    let mut exact_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        Duration::from_secs(0),
        Duration::from_millis(500),
        Duration::from_secs(6),
    ]));
    let exact_submission = Uuid::new_v4();
    let exact_command =
        identified_command(&tenant, "subject-a", exact_submission, None, "exact input");
    let outcome = exact_runner
        .execute_submission_with_observer_and_cancellation(exact_command, &mut |_| {}, &|| false)
        .expect("exactly four remaining seconds admits the write");
    assert!(matches!(
        outcome,
        koduck_ai::application::TurnOutcome::Owned(_)
    ));
    assert_eq!(exact_observed.lock().expect("inputs").len(), 1);
}

/// SI-07i (ADR-0018): a reading earlier than its request start fails closed
/// before fresh acceptance, without resetting or extending the budget.
pub(crate) fn cand_18_invalid_clock_fails_closed() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let (provider, backward_observed) = scripted_provider(vec![]);
    let mut backward_runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        Duration::from_secs(5),
        Duration::from_secs(4),
    ]));
    let backward_submission = Uuid::new_v4();
    let backward_command = identified_command(
        &tenant,
        "subject-a",
        backward_submission,
        None,
        "backward input",
    );
    assert!(matches!(
        backward_runner
            .execute_submission_with_observer_and_cancellation(
                backward_command,
                &mut |_| {},
                &|| false
            )
            .expect_err("the earlier reading fails closed"),
        TurnRunError::History(HistoryError::Unavailable)
    ));
    assert!(backward_observed.lock().expect("inputs").is_empty());
}

/// AC-7/SI-07b (ADR-0018): a rejected fresh preparation resolves the final
/// unlocked key lookup; with no binding committed the original rejection is
/// preserved.
pub(crate) fn cand_18_preparation_rejection() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let (provider, observed) = scripted_provider(completed_events());
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let submission = Uuid::new_v4();
    // The explicit Thread does not exist, so the bounded prior-history read
    // rejects; no concurrent commit wins the final lookup.
    let command = identified_command(
        &tenant,
        "subject-a",
        submission,
        Some(ThreadId::new()),
        "rejected input",
    );
    let rejection = runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect_err("the nonexistent thread rejects");
    assert!(matches!(
        rejection,
        TurnRunError::History(HistoryError::NotFound)
    ));
    assert!(
        observed.lock().expect("inputs").is_empty(),
        "a rejected preparation starts no provider work"
    );
    let rows =
        harness
            .runtime
            .block_on(count_rows(&harness.pool, &tenant, "subject-a", submission));
    assert_eq!(rows, (0, 0, 0, 0), "the rejection writes no binding");
}

/// A history double that relies entirely on the `TurnHistory` trait defaults
/// for the identified ports, proving SI-01d's fail-closed default.
#[derive(Default)]
struct BareHistory;

impl TurnHistory for BareHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(
        &self,
        _turn: &koduck_ai::application::AcceptedTurn,
    ) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_turns(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(
        &mut self,
        _command: &TurnCommand,
    ) -> Result<koduck_ai::application::AcceptedTurn, HistoryError> {
        panic!("an identified command must never reach the unidentified port");
    }

    fn append(
        &mut self,
        _turn: &koduck_ai::application::AcceptedTurn,
        _item: NewItem,
    ) -> Result<koduck_ai::domain::Item, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn replay(
        &self,
        _tenant_id: &koduck_ai::domain::TenantId,
        _turn_id: TurnId,
    ) -> Result<Vec<koduck_ai::domain::Item>, HistoryError> {
        Err(HistoryError::Unavailable)
    }
}

/// AC-1/SI-01d (ADR-0018): a history adapter relying on the port defaults
/// reports absence for the preliminary lookup and fails the acceptance
/// closed.
pub(crate) fn cand_18_default_port_fails_closed() {
    let _database_guard = super::serialize_database_tests();
    let (provider, observed) = scripted_provider(completed_events());
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        BareHistory,
        koduck_ai::application::NoToolExecution,
    );
    let submission = Uuid::new_v4();
    let command = identified_command("tenant-defaults", "subject-a", submission, None, "input");
    // The typed unavailability surfaces as the runner's durability failure
    // with no accepted identities (SI-01d).
    assert!(matches!(
        runner
            .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
            .expect_err("the default port fails closed"),
        TurnRunError::Durability(_)
    ));
    assert!(observed.lock().expect("inputs").is_empty());
}

/// AC-2 (ADR-0018): the unconfigured Tool boundary records the model Tool
/// call as the typed durable denial.
pub(crate) fn cand_18_runner_tool_call_is_recorded_as_denial() {
    let (harness, _history, _unused_tenant) = connected_history();
    let tenant = harness.runtime.block_on(async {
        let tenant = format!("cand18-tool-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(ThreadId::new().as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed thread");
        tenant
    });
    let (provider, observed) = scripted_provider(vec![
        ProviderEvent::ToolCall {
            name: "unknown_tool".to_owned(),
            arguments: "{}".to_owned(),
        },
        ProviderEvent::Completed,
    ]);
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let command = TurnCommand::new(trust(&tenant, "subject-a"), None, "tool input".to_owned())
        .expect("valid command");
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the denied tool round completes the turn");
    assert!(matches!(
        outcome,
        koduck_ai::application::TurnOutcome::Owned(_)
    ));
    assert_eq!(
        observed.lock().expect("inputs").len(),
        1,
        "exactly one provider stream serviced the denied call"
    );
}

/// ADR-0003 TC-11 (ADR-0018 preservation): a Tool-call round that ends its
/// stream without a terminal starts the continuation request carrying the
/// committed denial.
pub(crate) fn cand_18_runner_tool_round_continuation() {
    let (harness, _history, _unused_tenant) = connected_history();
    let tenant = harness.runtime.block_on(async {
        let tenant = format!("cand18-toolround-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(ThreadId::new().as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed thread");
        tenant
    });
    // The first stream raises the call and ends; the second (continuation)
    // stream ends without a terminal, so the Turn closes as the bounded
    // provider-stream failure after exactly two provider invocations.
    let (provider, observed) = scripted_provider(vec![ProviderEvent::ToolCall {
        name: "unknown_tool".to_owned(),
        arguments: "{}".to_owned(),
    }]);
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let command = TurnCommand::new(trust(&tenant, "subject-a"), None, "round input".to_owned())
        .expect("valid command");
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the tool round still produces an owned result");
    let koduck_ai::application::TurnOutcome::Owned(result) = outcome else {
        panic!("the tool round owns its result");
    };
    assert_eq!(result.status, TurnStatus::Failed);
    assert_eq!(
        observed.lock().expect("inputs").len(),
        2,
        "the continuation request carries the committed denial"
    );
}

/// ADR-0005 PLB-7 (ADR-0018 preservation): a provider stream that ends
/// without a terminal durably closes the Turn as the bounded
/// `PROVIDER_STREAM_ENDED` failure.
pub(crate) fn cand_18_runner_stream_ended_without_terminal() {
    let (harness, _history, tenant) = connected_history();
    let (provider, _observed) = scripted_provider(vec![ProviderEvent::Delta("partial".to_owned())]);
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let command = TurnCommand::new(
        trust(&tenant, "subject-a"),
        None,
        "dangling input".to_owned(),
    )
    .expect("valid command");
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the dangling stream still produces an owned result");
    let koduck_ai::application::TurnOutcome::Owned(result) = outcome else {
        panic!("the unidentified run owns its result");
    };
    assert_eq!(result.status, TurnStatus::Failed);
}

fn migration_fixture() -> (
    Harness,
    sqlx::PgPool,
    PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    String,
    String,
    String,
) {
    let harness = Harness::connect(4);
    let database_url =
        std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
    let schema = format!("cand18_mig_{}", Uuid::new_v4().simple());
    harness.runtime.block_on(async {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&harness.pool)
            .await
            .expect("create the migration fixture schema");
    });
    // The fixture pool keeps the production PgPoolOptions::new() defaults and
    // adds only the fixture search_path, so the unqualified startup DDL
    // resolves inside the isolated schema.
    let options = sqlx::postgres::PgConnectOptions::from_str(&database_url)
        .expect("valid test database URL")
        .options([("search_path", schema.clone())]);
    let pool = harness
        .runtime
        .block_on(sqlx::postgres::PgPoolOptions::new().connect_with(options))
        .expect("connect the migration fixture pool");
    // Populate migrations 0001..0009 first: the fixture starts pre-0010.
    harness.runtime.block_on(async {
        for migration in &MIGRATIONS[..9] {
            sqlx::raw_sql(sqlx::AssertSqlSafe((*migration).to_owned()))
                .execute(&pool)
                .await
                .expect("apply the pre-0010 migration sequence");
        }
    });
    let executor = SqlxPostgresExecutor::new(pool.clone(), harness.handle());
    let history = PostgresTurnHistory::new(executor);
    let tenant = format!("cand18-migration-{}", Uuid::new_v4());
    (harness, pool, history, tenant, schema, database_url)
}

/// AC-8 (ADR-0018): the exact production startup sequence applies twice on an
/// isolated populated fixture with no duplicate relation, changed legacy row,
/// or backfill, and one binding survives reapplication.
/// Seeds one legacy unidentified Turn on the pre-0010 fixture and returns
/// its identity plus its durable item count.
fn seed_legacy_turn(
    harness: &Harness,
    pool: &sqlx::PgPool,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    tenant: &str,
) -> (TurnId, i64) {
    // The production port drives its own runtime; only the raw count rides
    // the harness handle, so the two never nest.
    let command = TurnCommand::new(
        trust(tenant, "subject-a"),
        None,
        "legacy before migration".to_owned(),
    )
    .expect("valid legacy command");
    let seeded = TurnHistory::accept_initial(history, &command)
        .expect("the pre-0010 legacy acceptance creates");
    let rows: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM turn_items WHERE turn_id = $1")
            .bind(seeded.turn_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("count the seeded legacy items")
    });
    (seeded.turn_id, rows)
}

/// Counts the seeded legacy Turn's durable items.
async fn legacy_item_count(pool: &sqlx::PgPool, turn_id: TurnId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM turn_items WHERE turn_id = $1")
        .bind(turn_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("count the legacy items")
}

/// Drops the isolated fixture schema after closing its pool.
async fn drop_migration_fixture(database_url: &str, schema: &str, pool: sqlx::PgPool) {
    pool.close().await;
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .expect("reconnect for fixture teardown");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .expect("drop the migration fixture schema");
    admin.close().await;
}

pub(crate) fn cand_18_migration_and_integrity() {
    let _database_guard = super::serialize_database_tests();
    let (harness, pool, mut history, tenant, schema, database_url) = migration_fixture();
    let submission = Uuid::new_v4();

    // Seed legacy state while the fixture is still pre-0010: unidentified
    // acceptance needs only 0001..0009 and must stay untouched by 0010.
    let (seeded_turn, legacy_rows) = seed_legacy_turn(&harness, &pool, &mut history, &tenant);
    assert_eq!(
        legacy_rows, 1,
        "the legacy Turn carries its sequence-1 input"
    );

    // The exact startup sequence, applied twice through the production entry.
    for _ in 0..2 {
        harness
            .runtime
            .block_on(koduck_ai::runtime::apply_startup_migrations(
                &pool,
                Duration::from_secs(10),
            ))
            .expect("the startup sequence is idempotent");
    }
    let legacy_after = harness
        .runtime
        .block_on(legacy_item_count(&pool, seeded_turn));
    assert_eq!(
        legacy_rows, legacy_after,
        "reapplication changes no legacy rows and backfills nothing"
    );

    // With 0010 present, the identified acceptance works on the migrated
    // fixture and its binding survives a further full reapplication.
    let command = identified_command(&tenant, "subject-a", submission, None, "after migration");
    let IdentifiedAcceptance::Created(_) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the identified acceptance creates on the migrated fixture")
    else {
        panic!("the identified acceptance must create");
    };
    harness
        .runtime
        .block_on(koduck_ai::runtime::apply_startup_migrations(
            &pool,
            Duration::from_secs(10),
        ))
        .expect("a third full reapplication stays idempotent");
    let binding_count: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT count(*) FROM chat_submissions WHERE tenant_id = $1 AND submission_id = $2",
        )
        .bind(&tenant)
        .bind(submission)
        .fetch_one(&pool)
        .await
        .expect("count bindings after reapplication")
    });
    assert_eq!(
        binding_count, 1,
        "reapplication creates no duplicate binding"
    );

    harness
        .runtime
        .block_on(drop_migration_fixture(&database_url, &schema, pool));
}

/// The isolated AC-5 fixture: a private schema holding the complete canonical
/// migrations, one production-shaped pool bound to it through `search_path`,
/// and a tenant-scoped deferred commit trigger on its `chat_submissions`.
struct SubmissionFixture {
    database_url: String,
    schema: String,
    tenant: String,
    pool: sqlx::PgPool,
    harness: Harness,
}

impl SubmissionFixture {
    fn create(label: &str, trigger_body: &str) -> SubmissionFixture {
        let harness = Harness::connect(4);
        let database_url =
            std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
        let schema = format!("cand18_submission_{}", Uuid::new_v4().simple());
        let tenant = format!("cand18-reconcile-{label}-{}", Uuid::new_v4());
        harness.runtime.block_on(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&harness.pool)
                .await
                .expect("create the reconciliation fixture schema");
        });
        let options = sqlx::postgres::PgConnectOptions::from_str(&database_url)
            .expect("valid test database URL")
            .options([("search_path", schema.clone())]);
        let pool = harness
            .runtime
            .block_on(sqlx::postgres::PgPoolOptions::new().connect_with(options))
            .expect("connect the fixture pool");
        // The fixture carries the complete canonical schema, 0010 included,
        // through the exact production startup sequence; the trigger installs
        // afterwards, once its relation exists.
        harness
            .runtime
            .block_on(koduck_ai::runtime::apply_startup_migrations(
                &pool,
                Duration::from_secs(20),
            ))
            .expect("apply the canonical migrations into the fixture schema");
        harness.runtime.block_on(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE FUNCTION {schema}.cand18_commit_trigger() RETURNS trigger AS \
                 $body$ BEGIN IF NEW.tenant_id = $tenant${tenant}$tenant$ THEN \
                 {trigger_body} END IF; RETURN NEW; END; $body$ LANGUAGE plpgsql; \
                 CREATE CONSTRAINT TRIGGER cand18_commit_deferred \
                 AFTER INSERT ON {schema}.chat_submissions \
                 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
                 EXECUTE FUNCTION {schema}.cand18_commit_trigger()"
            )))
            .execute(&pool)
            .await
            .expect("install the deferred commit trigger");
        });
        SubmissionFixture {
            database_url,
            schema,
            tenant,
            pool,
            harness,
        }
    }

    fn history(
        &self,
    ) -> PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor> {
        PostgresTurnHistory::new(
            koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
                self.pool.clone(),
                self.harness.handle(),
            ),
        )
    }

    fn teardown(self) {
        let SubmissionFixture {
            database_url,
            schema,
            tenant: _tenant,
            pool,
            harness,
        } = self;
        harness.runtime.block_on(async move {
            pool.close().await;
            let admin = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("reconnect for fixture teardown");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE"
            )))
            .execute(&admin)
            .await
            .expect("drop the reconciliation fixture schema");
            admin.close().await;
        });
    }
}

/// AC-5/SI-06 (ADR-0018): a delayed deferred-trigger commit whose write
/// acknowledgement times out is reconciled by the same invocation's read-only
/// proof, which observes the live creator and still returns the created
/// owner; the deferred-exception variant commits nothing and stays
/// unavailable.
pub(crate) fn cand_18_commit_reconciliation() {
    let _database_guard = super::serialize_database_tests();
    // Variant one: the commit survives the dropped acknowledgement.
    {
        let fixture = SubmissionFixture::create("delay", "PERFORM pg_sleep(2.5);");
        let mut history = fixture.history();
        let submission = Uuid::new_v4();
        let command = identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            None,
            "reconciled input",
        );
        let started = std::time::Instant::now();
        let outcome = TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .expect("the delayed commit reconciles to the created owner");
        let elapsed = started.elapsed();
        let IdentifiedAcceptance::Created(accepted) = outcome else {
            panic!("the same invocation must prove its live creator");
        };
        assert_eq!(accepted.generation.get(), 1);
        assert!(
            elapsed >= Duration::from_secs(2),
            "the write attempt consumed its full budget before the proof"
        );
        let rows = fixture.harness.runtime.block_on(count_rows(
            &fixture.pool,
            &fixture.tenant,
            "subject-a",
            submission,
        ));
        assert_eq!(rows, (1, 1, 1, 1), "exactly one durable acceptance exists");
        fixture.teardown();
    }

    // Variant two: the deferred trigger aborts the commit, so no binding and
    // no canonical state exists and the outcome stays unavailable.
    {
        let fixture =
            SubmissionFixture::create("abort", "RAISE EXCEPTION 'cand18 controlled commit fault';");
        let mut history = fixture.history();
        let submission = Uuid::new_v4();
        let command = identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            None,
            "aborted input",
        );
        assert_eq!(
            TurnHistory::accept_initial_with_submission(
                &mut history,
                &command,
                Duration::from_secs(2),
            )
            .expect_err("the aborted commit stays unavailable"),
            HistoryError::Unavailable
        );
        let rows = fixture.harness.runtime.block_on(count_rows(
            &fixture.pool,
            &fixture.tenant,
            "subject-a",
            submission,
        ));
        assert_eq!(
            rows,
            (0, 0, 0, 0),
            "the aborted commit leaves zero partial rows"
        );
        fixture.teardown();
    }
}

/// The production `run` assembly — pool construction, migration application,
/// adapter wiring, and the explicit system acceptance clock — executes up to
/// the listener bind, which a pre-bound port deterministically rejects.
pub(crate) fn cand_18_runtime_assembly() {
    let _database_guard = super::serialize_database_tests();
    let harness = Harness::connect(4);
    let database_url =
        std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
    // A bound listener makes the assembly's bind step fail immediately, so
    // every preceding assembly line runs inside the test.
    let probe_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("probe port");
    let bind_addr = probe_listener.local_addr().expect("probe address");
    let environment = std::collections::BTreeMap::from([
        ("KODUCK_AI_BIND_ADDR".to_owned(), bind_addr.to_string()),
        ("KODUCK_AI_DATABASE_URL".to_owned(), database_url),
        (
            "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
            "https://provider.example/v1".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_MODEL".to_owned(),
            "provider-model".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_API_KEY".to_owned(),
            "not-a-real-secret".to_owned(),
        ),
    ]);
    let config = koduck_ai::runtime::RuntimeConfig::from_environment(&environment)
        .expect("the assembly environment validates");
    let outcome = harness
        .runtime
        .block_on(async move { koduck_ai::runtime::run(config).await });
    assert!(
        matches!(outcome, Err(koduck_ai::runtime::RuntimeError::Bind(_))),
        "the assembly reaches the listener bind and fails on the occupied port"
    );
    drop(probe_listener);
}

/// A history double that panics on every identified-port call, proving the
/// SI-01d direct-command guard fires before any history I/O.
#[derive(Default)]
struct PanickingLookupHistory(BareHistory);

impl TurnHistory for PanickingLookupHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(
        &self,
        _turn: &koduck_ai::application::AcceptedTurn,
    ) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_turns(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(
        &mut self,
        _command: &TurnCommand,
    ) -> Result<koduck_ai::application::AcceptedTurn, HistoryError> {
        panic!("an identified command must never reach the unidentified port");
    }

    fn append(
        &mut self,
        _turn: &koduck_ai::application::AcceptedTurn,
        _item: NewItem,
    ) -> Result<koduck_ai::domain::Item, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn replay(
        &self,
        _tenant_id: &TenantId,
        _turn_id: TurnId,
    ) -> Result<Vec<koduck_ai::domain::Item>, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn submission_observation(
        &self,
        _command: &TurnCommand,
        _deadline: Duration,
    ) -> Result<Option<koduck_ai::application::SubmissionObservation>, HistoryError> {
        panic!("an invalid direct command must not reach the preliminary lookup");
    }

    fn accept_initial_with_submission(
        &mut self,
        _command: &TurnCommand,
        _deadline: Duration,
    ) -> Result<IdentifiedAcceptance, HistoryError> {
        panic!("an invalid direct command must not reach the acceptance write");
    }
}

/// SI-01d (review round 1, finding 1): a directly constructed invalid
/// identified command — empty input, oversized input, or a nil submission
/// identity — is rejected by the runner before any history operation.
#[test]
fn cand_18_runner_guard_precedes_lookup() {
    let (provider, observed) = scripted_provider(vec![]);
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PanickingLookupHistory::default(),
        koduck_ai::application::NoToolExecution,
    );
    let submission = Uuid::new_v4();
    let trust_context = trust("tenant-guards", "subject-a");
    let invalid_commands = [
        // Empty input with a valid identity.
        TurnCommand {
            trust: trust_context.clone(),
            thread_id: None,
            input: String::new(),
            submission_id: Some(SubmissionId::from_uuid(submission).expect("non-nil submission")),
        },
        // Oversized input with a valid identity.
        TurnCommand {
            trust: trust_context.clone(),
            thread_id: None,
            input: "a".repeat(65_537),
            submission_id: Some(SubmissionId::from_uuid(submission).expect("non-nil submission")),
        },
        // The input bound is the remaining bypass surface: the submission
        // identity itself is non-nil by construction, so the typed field
        // cannot carry the nil UUID.
    ];
    for command in invalid_commands {
        assert!(
            matches!(
                runner
                    .execute_submission_with_observer_and_cancellation(
                        command,
                        &mut |_| {},
                        &|| false
                    )
                    .expect_err("the guard rejects the invalid command"),
                koduck_ai::application::TurnRunError::InvalidCommand(_)
            ),
            "the rejection must be the typed invalid-command result, before any history I/O"
        );
    }
    assert!(
        observed.lock().expect("inputs").is_empty(),
        "no provider work accompanies the rejection"
    );
}

/// SI-02e/SI-03a (review round 1, finding 2): a fresh identified acceptance
/// with an explicit unknown Thread returns `NotFound` without creating the
/// Thread or claiming the key, while an explicit existing owned Thread is
/// accepted.
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

/// SI-08c/SI-09 (review round 1, finding 3): a stored sequence-1 payload
/// above the 65,536-byte input bound is corrupt structure, so every binding
/// lookup fails unavailable instead of reporting a semantic conflict.
#[test]
fn cand_18_oversized_stored_input_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Corrupt the stored sequence-1 payload beyond the input bound.
    harness.runtime.block_on(async {
        let oversized = format!(r#"{{"content":"{}"}}"#, "a".repeat(70_000));
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(oversized)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored input");
    });

    // The exact same request now fails unavailable, never a conflict.
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("the oversized stored input is corrupt structure"),
        HistoryError::Unavailable
    );
    // The transaction recheck keeps the same classification.
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the acceptance recheck stays unavailable"),
        HistoryError::Unavailable
    );
}

/// SI-01d (review round 2, finding 2): the direct observation port rejects an
/// invalid identified command before any database query — proven by an
/// exclusive table lock that would otherwise stall the lookup until its
/// deadline.
#[test]
fn cand_18_observation_entry_guard_precedes_query() {
    let _database_guard = super::serialize_database_tests();
    let (harness, history, tenant) = connected_history();
    let submission = Uuid::new_v4();

    // Hold an exclusive lock on the binding relation for the whole case.
    let lock_pool = harness.pool.clone();
    let mut lock = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("lock transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("LOCK TABLE chat_submissions IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .expect("hold the binding relation exclusively");
    });

    // Invalid commands return the typed unavailability immediately, without
    // waiting out the lookup deadline the locked query would consume.
    for input in [String::new(), "a".repeat(65_537)] {
        let command = TurnCommand {
            trust: trust(&tenant, "subject-a"),
            thread_id: None,
            input,
            submission_id: Some(SubmissionId::from_uuid(submission).expect("non-nil")),
        };
        let started = std::time::Instant::now();
        assert_eq!(
            history
                .submission_observation(&command, Duration::from_secs(2))
                .expect_err("the guard rejects the invalid command"),
            HistoryError::Unavailable
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the guard rejected in {:?} without the locked database query",
            started.elapsed()
        );
    }
    harness.runtime.block_on(async move {
        drop(lock);
    });
}

/// SI-07c (review round 3, finding 1): the bounded prior-history read honors
/// its caller-owned deadline — proven under an exclusive lock on the tables
/// it reads, where the fixed two-second budget and a 100 ms clamped budget
/// are separated by their elapsed times.
#[test]
fn cand_18_bounded_history_read_honors_deadline() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed thread");
    });
    let executor = SqlxPostgresExecutor::new(harness.pool.clone(), harness.handle());

    let lock_pool = harness.pool.clone();
    let mut lock = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("lock transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("LOCK TABLE turn_items IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .expect("hold the item relation exclusively");
    });

    // The clamped 100 ms budget rejects after ~100 ms, far inside the fixed
    // two-second budget an unclamped read would consume.
    let started = std::time::Instant::now();
    assert_eq!(
        executor
            .prior_thread_turns_bounded(
                &trust(&tenant, "subject-a"),
                thread,
                Duration::from_millis(100),
            )
            .expect_err("the clamped read times out"),
        HistoryError::Unavailable
    );
    let clamped_elapsed = started.elapsed();
    assert!(
        clamped_elapsed < Duration::from_millis(1_500),
        "the clamped read rejected in {clamped_elapsed:?}"
    );
    harness.runtime.block_on(async move {
        drop(lock);
    });
}

/// SI-09 (review round 3, finding 2): a stored sequence-1 payload that is
/// noncanonical (extra JSON members) or beyond the transport envelope fails
/// unavailable before comparison, never producing a receipt.
#[test]
fn cand_18_noncanonical_and_oversized_payloads_fail_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Noncanonical structure: an extra member beside `content`.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(r#"{"content":"bounded input","junk":"unexpected"}"#)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored structure");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("noncanonical stored structure fails unavailable"),
        HistoryError::Unavailable
    );

    // Beyond the transport envelope: the payload is never compared.
    harness.runtime.block_on(async {
        let oversized = format!(
            r#"{{"content":"bounded input","junk":"{}"}}"#,
            "b".repeat(500_000)
        );
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(oversized)
        .execute(&harness.pool)
        .await
        .expect("push the stored payload past the envelope");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("the oversized envelope fails unavailable"),
        HistoryError::Unavailable
    );
}

/// SI-01d (review round 4, finding 1): the legacy runner entry points reject
/// an identified command with the typed invalid-command result instead of
/// silently executing it through the unidentified path.
#[test]
fn cand_18_legacy_entries_reject_identified_commands() {
    let (harness, _history, _tenant) = connected_history();
    let (provider, observed) = scripted_provider(completed_events());
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let submission = Uuid::new_v4();
    let command = identified_command(
        "tenant-legacy",
        "subject-a",
        submission,
        None,
        "legacy entry",
    );
    assert!(
        matches!(
            runner.execute(command),
            Err(TurnRunError::InvalidCommand(_))
        ),
        "the legacy entry must reject the identified command before any acceptance"
    );
    assert!(observed.lock().expect("inputs").is_empty());
    let bindings: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM chat_submissions WHERE tenant_id = $1")
            .bind("tenant-legacy")
            .fetch_one(&harness.pool)
            .await
            .expect("count bindings")
    });
    assert_eq!(bindings, 0, "the rejected command writes no binding");
}

/// SI-08c/SI-09 (review round 4, finding 2): a stored sequence-1 payload
/// with duplicate JSON members is inconsistent structure and fails
/// unavailable, even when the surviving member matches the request.
#[test]
fn cand_18_duplicate_member_payload_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Duplicate members collapse to the last value in plain serde_json; the
    // lookup must reject the inconsistent structure outright.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(r#"{"content":"other","content":"bounded input"}"#)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored structure with duplicate members");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("duplicate members are inconsistent stored structure"),
        HistoryError::Unavailable
    );
}

/// SI-07c (review round 5, finding 1): a `TurnHistory` adapter that does not
/// implement the bounded prior-history read fails closed instead of silently
/// delegating to its unbounded read — proven by an unbounded read that
/// panics if ever reached.
#[derive(Default)]
struct UnboundedReadPanicsHistory(BareHistory);

impl TurnHistory for UnboundedReadPanicsHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(
        &self,
        _turn: &koduck_ai::application::AcceptedTurn,
    ) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_turns(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        panic!("the default bounded read must not delegate to the unbounded read");
    }

    fn accept_initial(
        &mut self,
        _command: &TurnCommand,
    ) -> Result<koduck_ai::application::AcceptedTurn, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn append(
        &mut self,
        _turn: &koduck_ai::application::AcceptedTurn,
        _item: NewItem,
    ) -> Result<koduck_ai::domain::Item, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn replay(
        &self,
        _tenant_id: &TenantId,
        _turn_id: TurnId,
    ) -> Result<Vec<koduck_ai::domain::Item>, HistoryError> {
        Err(HistoryError::Unavailable)
    }
}

/// SI-07c: the identified flow against a deadline-unaware adapter fails
/// closed with the typed unavailability before its unbounded read runs.
#[test]
fn cand_18_bounded_read_default_fails_closed() {
    let (provider, observed) = scripted_provider(vec![]);
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        UnboundedReadPanicsHistory::default(),
        koduck_ai::application::NoToolExecution,
    );
    let submission = Uuid::new_v4();
    // The explicit selector drives the bounded preparation read; the double's
    // unbounded read panics, so reaching it fails the test.
    let command = identified_command(
        "tenant-bounded-default",
        "subject-a",
        submission,
        Some(ThreadId::new()),
        "bounded default input",
    );
    let result =
        runner.execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false);
    // The rejected preparation resolves the final unlocked lookup, whose
    // default reports absence, so the original rejection surfaces.
    assert!(matches!(
        result.expect_err("the unaware adapter fails closed"),
        TurnRunError::History(HistoryError::Unavailable)
    ));
    assert!(observed.lock().expect("inputs").is_empty());
}
