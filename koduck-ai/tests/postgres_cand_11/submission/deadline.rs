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
    TurnHistory, TurnOutcome, TurnRunError, TurnRunner,
};
use koduck_ai::domain::{SubmissionId, TenantId, ThreadId, TrustContext, TurnId};

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

/// SI-01d (review round 7, finding 1): the default observation entry fails
/// closed for an invalid identified command instead of reporting absence.
#[test]
fn cand_18_default_observation_entry_guards_commands() {
    let history = BareHistory;
    let command = TurnCommand {
        trust: TrustContext::new(
            TenantId::new("tenant-default-observation".to_owned()).expect("valid tenant"),
            "subject-a",
        )
        .expect("valid trust"),
        thread_id: None,
        input: String::new(),
        submission_id: Some(SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil")),
    };
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(1))
            .expect_err("the default entry rejects the invalid command"),
        HistoryError::Unavailable
    );
}

/// SI-01d/SI-09 (review round 15): the direct acceptance entry rejects a
/// directly constructed oversized command with the typed unavailability and
/// no database I/O. The guard now runs before the entry's input clone, so the
/// invalid command is cheaply refused instead of doubling its footprint; the
/// clone-versus-validate memory ordering itself is source-evident and cannot
/// be red-tested here because the workspace's non-waivable `unsafe_code`
/// forbid rules out an allocation-probe double.
#[test]
fn cand_18_acceptance_entry_rejects_oversized_commands() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let command = TurnCommand {
        trust: TrustContext::new(TenantId::new(tenant).expect("valid tenant"), "subject-a")
            .expect("valid trust"),
        thread_id: None,
        input: "x".repeat(65_537),
        submission_id: Some(SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil")),
    };
    assert_eq!(
        koduck_ai::adapters::history::postgres::PostgresExecutor::accept_initial_with_submission(
            &harness.executor(),
            &command,
            Duration::from_secs(2),
        )
        .expect_err("the oversized command is rejected at the direct entry guard"),
        HistoryError::Unavailable
    );
}

/// A history double recording every identified-port deadline so the staged
/// acceptance clock's clamp and reservation arithmetic is observable at the
/// consuming port (AC-6/SI-07c, SI-07i).
#[derive(Clone, Default)]
struct RecordingHistory {
    lookups: std::sync::Arc<std::sync::Mutex<Vec<Duration>>>,
    writes: std::sync::Arc<std::sync::Mutex<Vec<Duration>>>,
}

impl RecordingHistory {
    fn lookups(&self) -> Vec<Duration> {
        self.lookups.lock().expect("lookup log").clone()
    }

    fn writes(&self) -> Vec<Duration> {
        self.writes.lock().expect("write log").clone()
    }
}

impl TurnHistory for RecordingHistory {
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
        deadline: Duration,
    ) -> Result<Option<koduck_ai::application::SubmissionObservation>, HistoryError> {
        self.lookups.lock().expect("lookup log").push(deadline);
        Ok(None)
    }

    fn accept_initial_with_submission(
        &mut self,
        command: &TurnCommand,
        deadline: Duration,
    ) -> Result<koduck_ai::application::IdentifiedAcceptance, HistoryError> {
        self.writes.lock().expect("write log").push(deadline);
        let receipt = koduck_ai::application::SubmissionObservation {
            submission_id: command
                .submission_id
                .expect("the runner validated the identity"),
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
        };
        Ok(koduck_ai::application::IdentifiedAcceptance::Existing(
            receipt,
        ))
    }
}

/// AC-6/SI-07c (ADR-0018): pre-write reads clamp to the remaining acceptance
/// time while a permitted acceptance write receives its full reserved
/// two-second budget — proven through the staged manual clock and a recording
/// history double at the consuming port.
#[test]
fn cand_18_pre_write_read_clamp() {
    clamped_lookup_starts_no_write();
    permitted_write_keeps_full_budgets();
}

/// Clamped lookup: 9.5 s elapsed of the 10 s deadline leaves 500 ms, so the
/// preliminary read receives 500 ms, and the 500 ms remaining at the write
/// gate starts no write.
fn clamped_lookup_starts_no_write() {
    let (provider, observed) = scripted_provider(vec![]);
    let history = RecordingHistory::default();
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        history.clone(),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        Duration::from_secs(0),
        Duration::from_millis(9_500),
        Duration::from_millis(9_500),
    ]));
    let command = identified_command(
        "tenant-clamp",
        "subject-a",
        Uuid::new_v4(),
        None,
        "clamped input",
    );
    assert!(matches!(
        runner
            .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
            .expect_err("the clamped budget starts no write"),
        TurnRunError::History(HistoryError::Unavailable)
    ));
    assert_eq!(
        history.lookups(),
        vec![Duration::from_millis(500)],
        "the preliminary read is clamped to the remaining 500 ms"
    );
    assert!(
        history.writes().is_empty(),
        "less than four remaining seconds starts no write"
    );
    assert!(
        observed.lock().expect("inputs").is_empty(),
        "no observation outcome starts provider work"
    );
}

/// Full budgets: 1 s elapsed clamps nothing, and the permitted write at
/// exactly four remaining seconds receives the full reserved 2 s attempt.
fn permitted_write_keeps_full_budgets() {
    let (provider, _observed) = scripted_provider(vec![]);
    let history = RecordingHistory::default();
    let mut runner = koduck_ai::runtime::compose_production_runner(
        provider,
        history.clone(),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        Duration::from_secs(0),
        Duration::from_secs(1),
        Duration::from_secs(6),
    ]));
    let command = identified_command(
        "tenant-clamp",
        "subject-a",
        Uuid::new_v4(),
        None,
        "full input",
    );
    let outcome = runner
        .execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
        .expect("the full budget admits the write");
    assert!(matches!(outcome, TurnOutcome::Observed(_)));
    assert_eq!(
        history.lookups(),
        vec![Duration::from_secs(2)],
        "the unclamped preliminary read keeps its full two-second budget"
    );
    assert_eq!(
        history.writes(),
        vec![koduck_ai::application::WRITE_BUDGET],
        "a permitted write receives its full reserved budget, never compressed by the deadline"
    );
    assert_eq!(history.writes().len(), 1);
}

/// AC-6/SI-07i (ADR-0018): runner cloning and Tool composition retain the
/// clock dependency while each identified request owns its separate
/// start/budget value — a clone's reading earlier than its own request start
/// fails closed, proving no budget is shared or reset.
/// A cloneable no-op provider so the runner's `Clone` composition — the
/// SI-07i clone/Tool retention case — is exercisable with a shared clock.
#[derive(Clone)]
struct CloneableProvider;

impl koduck_ai::application::ModelProvider for CloneableProvider {
    fn stream(
        &mut self,
        _input: koduck_ai::application::ModelInput,
    ) -> Result<koduck_ai::application::ProviderStream<'_>, koduck_ai::application::ProviderError>
    {
        panic!("observation outcomes never invoke the provider")
    }
}

#[test]
fn cand_18_clock_composition_separate_budgets() {
    let history = RecordingHistory::default();
    let composed = koduck_ai::runtime::compose_production_runner(
        CloneableProvider,
        history.clone(),
        koduck_ai::application::NoToolExecution,
    )
    .with_acceptance_clock(ManualClock::staged(&[
        // Request A on the Tool-composed runner: its own fresh budget.
        Duration::from_secs(0),
        Duration::from_millis(500),
        Duration::from_millis(600),
        // Request B on the clone: a start of 5 s and an earlier 3 s reading
        // fail closed before any lookup.
        Duration::from_secs(5),
        Duration::from_secs(3),
        // Request C on the clone: its own fresh budget from 5.5 s.
        Duration::from_millis(5_500),
        Duration::from_millis(5_700),
        Duration::from_millis(5_800),
    ]));

    // The Tool-composed runner retains the injected clock (request A works).
    let mut composed_runner = composed.clone();
    let outcome = execute_clone_request(&mut composed_runner, "first")
        .expect("the composed runner executes its own budget");
    assert!(matches!(outcome, TurnOutcome::Observed(_)));

    // The clone's separate budget: 3 s precedes its own 5 s start, so the
    // request fails closed without any lookup.
    let mut cloned = composed_runner.clone();
    assert!(matches!(
        execute_clone_request(&mut cloned, "backward")
            .expect_err("the clone's earlier reading fails closed"),
        TurnRunError::History(HistoryError::Unavailable)
    ));
    assert_eq!(
        history.lookups().len(),
        1,
        "the fail-closed clone request performed no lookup"
    );

    // A later request on the same clone derives a fresh budget from its own
    // start and proceeds normally.
    let outcome = execute_clone_request(&mut cloned, "third")
        .expect("the clone's later request uses its own budget");
    assert!(matches!(outcome, TurnOutcome::Observed(_)));
    assert_eq!(
        history.lookups(),
        vec![Duration::from_secs(2), Duration::from_secs(2)],
        "every request's lookup keeps the full clamped budget"
    );
    assert_eq!(history.writes().len(), 2);
}

/// Runs one identified request on the composed or cloned runner with the
/// shared no-op observer and cancellation closures.
fn execute_clone_request(
    runner: &mut TurnRunner<CloneableProvider, RecordingHistory>,
    input: &str,
) -> Result<TurnOutcome, TurnRunError> {
    let command = identified_command("tenant-compose", "subject-a", Uuid::new_v4(), None, input);
    runner.execute_submission_with_observer_and_cancellation(command, &mut |_| {}, &|| false)
}

/// A `PostgresExecutor` double that performs no validation of its own, so the
/// generic wrapper's direct-port guard (SI-01d) is the only barrier between
/// an invalid command and this executor's simulated I/O. It also records
/// every delegated deadline so a test can prove the wrapper clamps direct
/// budgets (SI-07c).
#[derive(Clone, Default)]
struct UnguardedExecutor {
    reached: std::sync::Arc<std::sync::atomic::AtomicBool>,
    delegated_deadlines: std::sync::Arc<std::sync::Mutex<Vec<Duration>>>,
}

fn receipt() -> koduck_ai::application::SubmissionObservation {
    koduck_ai::application::SubmissionObservation {
        submission_id: SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
    }
}

impl koduck_ai::adapters::history::postgres::PostgresExecutor for UnguardedExecutor {
    fn request_interrupt(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
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

    fn prior_thread_turns_bounded(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
        deadline: Duration,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        self.delegated_deadlines
            .lock()
            .expect("deadline log")
            .push(deadline);
        self.prior_thread_turns(trust, thread_id)
            .map_err(|_| HistoryError::Unavailable)
            .map(|mut turns| {
                turns.clear();
                turns
            })
    }

    fn accept_initial(
        &self,
        _command: &TurnCommand,
    ) -> Result<koduck_ai::application::AcceptedTurn, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn submission_observation(
        &self,
        _command: &TurnCommand,
        deadline: Duration,
    ) -> Result<Option<koduck_ai::application::SubmissionObservation>, HistoryError> {
        self.reached
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.delegated_deadlines
            .lock()
            .expect("deadline log")
            .push(deadline);
        Ok(Some(receipt()))
    }

    fn accept_initial_with_submission(
        &self,
        _command: &TurnCommand,
        attempt_budget: Duration,
    ) -> Result<koduck_ai::application::IdentifiedAcceptance, HistoryError> {
        self.reached
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.delegated_deadlines
            .lock()
            .expect("deadline log")
            .push(attempt_budget);
        Ok(koduck_ai::application::IdentifiedAcceptance::Existing(
            receipt(),
        ))
    }

    fn append(
        &self,
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
        Ok(Vec::new())
    }

    fn renew_lease(
        &self,
        _key: &koduck_ai::adapters::history::postgres::LeaseKey,
        _now_ms: u64,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn reconcile_expired(
        &self,
        _key: &koduck_ai::adapters::history::postgres::LeaseKey,
        _now_ms: u64,
        _timing: koduck_ai::adapters::history::postgres::LeaseTiming,
    ) -> Result<koduck_ai::adapters::history::postgres::ReconcileOutcome, HistoryError> {
        Err(HistoryError::Unavailable)
    }

    fn recover_failed(
        &self,
        _turn: &koduck_ai::application::AcceptedTurn,
        _timing: koduck_ai::adapters::history::postgres::LeaseTiming,
    ) -> Result<koduck_ai::adapters::history::postgres::RecoveryOutcome, HistoryError> {
        Err(HistoryError::Unavailable)
    }
}

/// SI-01d (review round 15's concurrently arriving finding on the wrapper):
/// the generic `PostgresTurnHistory` entry points validate an identified
/// command before delegating, so an empty, oversized, or identity-less
/// command never reaches a custom executor's I/O. The nil-identity form is
/// unrepresentable through the typed constructor and stays covered by the
/// validation's own defense-in-depth arm.
#[test]
fn cand_18_wrapper_guards_identified_commands_before_delegation() {
    let probe = UnguardedExecutor::default();
    let mut history =
        koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(probe.clone());
    let valid_trust = || {
        TrustContext::new(
            TenantId::new("tenant-wrapper-guards".to_owned()).expect("valid"),
            "subject-a",
        )
        .expect("valid trust")
    };
    let commands = [
        TurnCommand {
            trust: valid_trust(),
            thread_id: None,
            input: String::new(),
            submission_id: Some(SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil")),
        },
        TurnCommand {
            trust: valid_trust(),
            thread_id: None,
            input: "x".repeat(65_537),
            submission_id: Some(SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil")),
        },
        TurnCommand {
            trust: valid_trust(),
            thread_id: None,
            input: "wrapper input".to_owned(),
            submission_id: None,
        },
    ];
    for command in &commands {
        assert_eq!(
            history
                .submission_observation(command, Duration::from_secs(2))
                .expect_err("the wrapper guards the observation entry"),
            HistoryError::Unavailable
        );
        assert_eq!(
            TurnHistory::accept_initial_with_submission(
                &mut history,
                command,
                Duration::from_secs(2)
            )
            .expect_err("the wrapper guards the acceptance entry"),
            HistoryError::Unavailable
        );
    }
    assert!(
        !probe.reached.load(std::sync::atomic::Ordering::Relaxed),
        "no invalid command reaches a custom executor's I/O"
    );
}

/// SI-07c: the generic wrapper caps read deadlines while preserving short
/// reads, and always delegates the full reserved write/proof budget, even
/// when a direct caller supplies zero or less than two seconds.
#[test]
fn cand_18_wrapper_clamps_direct_budgets() {
    let probe = UnguardedExecutor::default();
    let mut history =
        koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(probe.clone());
    let command = TurnCommand {
        trust: TrustContext::new(
            TenantId::new("tenant-wrapper-budgets".to_owned()).expect("valid"),
            "subject-a",
        )
        .expect("valid trust"),
        thread_id: None,
        input: "clamped budget".to_owned(),
        submission_id: Some(SubmissionId::from_uuid(Uuid::new_v4()).expect("non-nil")),
    };
    let mut expected = Vec::new();
    for supplied in [
        Duration::ZERO,
        Duration::from_millis(500),
        koduck_ai::application::WRITE_BUDGET,
        Duration::from_secs(9),
    ] {
        history
            .submission_observation(&command, supplied)
            .expect("the observation delegates");
        TurnHistory::accept_initial_with_submission(&mut history, &command, supplied)
            .expect("the acceptance delegates");
        TurnHistory::prior_thread_turns_bounded(
            &history,
            &command.trust,
            command.thread_id.unwrap_or_default(),
            supplied,
        )
        .expect("the bounded history read delegates");
        expected.extend([
            supplied.min(koduck_ai::application::LOOKUP_BUDGET),
            koduck_ai::application::WRITE_BUDGET,
            supplied.min(koduck_ai::application::LOOKUP_BUDGET),
        ]);
    }
    assert_eq!(
        probe
            .delegated_deadlines
            .lock()
            .expect("deadline log")
            .as_slice(),
        expected.as_slice(),
        "reads retain short deadlines; settlement always receives its full budget"
    );
}
