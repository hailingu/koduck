// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 acceptance-clock and port-default cases: the remaining-time write
//! gate, the fail-closed earlier reading, rejected preparation, the
//! default-port fail-closed behaviors, and the bounded-history-read default.

use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::PostgresTurnHistory;
use koduck_ai::application::{
    AcceptanceClock, AcceptanceInstant, HistoryError, NewItem, PriorTurnHistory, TurnCommand,
    TurnHistory, TurnRunError,
};
use koduck_ai::domain::{TenantId, ThreadId, TrustContext, TurnId};

use super::runner::{completed_events, scripted_provider};
use super::{connected_history, count_rows, identified_command};

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
pub(super) struct BareHistory;

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
