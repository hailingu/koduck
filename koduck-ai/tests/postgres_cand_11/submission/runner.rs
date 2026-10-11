// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 production-runner identified-flow cases: created-owner execution,
//! exact-retry observation, cancellation branches, tool and stream
//! preservation, and the legacy-entry guards.

use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::PostgresTurnHistory;
use koduck_ai::application::{
    HistoryError, IdentifiedAcceptance, ModelInput, ModelProvider, NewItem, PriorTurnHistory,
    ProviderError, ProviderEvent, ProviderStream, TurnCommand, TurnHistory, TurnOutcome,
    TurnRunError,
};
use koduck_ai::domain::{SubmissionId, TenantId, ThreadId, TrustContext, TurnId, TurnStatus};

use super::deadline::BareHistory;
use super::{connected_history, count_rows, identified_command, trust};

pub(super) struct ScriptedProvider {
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

pub(super) fn scripted_provider(
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

pub(super) fn completed_events() -> Vec<koduck_ai::application::ProviderEvent> {
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
