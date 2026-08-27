// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Provider-neutral lifecycle orchestration and durable-before-visible ordering.

use std::time::{Duration, Instant};

use crate::domain::{Item, TenantId, TerminalOutcome, ThreadId, TrustContext, Turn, TurnId, Usage};

use super::delta_coalescer::DeltaCoalescer;
use super::ports::{
    AcceptedTurn, CommittedToolCall, HistoryError, ModelInput, ModelProvider, NewItem,
    NoToolExecution, ProviderEvent, ToolCallExecutor, ToolRound, TurnCommand, TurnHistory,
    TurnResult, TurnRunError, TurnStreamEvent,
};

pub(super) mod failure;
pub(super) mod tool_call;

use failure::{
    accept_appended_provider_item, enforce_provider_limit, history_failure, recover_append_failure,
};

use super::runner_terminals::{
    append_coalesced_deltas, append_provider_terminal, append_provider_terminal_observed,
    append_terminal, event_terminal_or_recover, flush_buffered_deltas, observe_item,
    publish_replayed_terminal,
};

/// Maximum frequency of persisted interruption checks during provider streams.
const INTERRUPTION_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Owns provider-neutral lifecycle transitions and durable-before-visible ordering.
///
/// `T` is the consumer-owned tool-execution boundary servicing model Tool
/// calls through C-5; the default [`NoToolExecution`] records every call as a
/// typed unavailability without executing it (ADR-0003 TC-13).
#[derive(Clone)]
pub struct TurnRunner<P, H, T = NoToolExecution> {
    provider: P,
    history: H,
    tools: T,
}

pub(super) struct ExecutionState {
    pub(super) published: Vec<Item>,
    /// Leading count of `published` items already sent to the observer.
    ///
    /// Tool-projection items are observed at their publish boundary while the
    /// call is still serviced, so the driving loop resumes observation at
    /// this watermark instead of re-observing them.
    pub(super) observed_len: usize,
    pub(super) usage: Usage,
    pub(super) lifecycle: Turn,
    pub(super) provider_item_count: usize,
    pub(super) provider_payload_bytes: usize,
    /// Every completed Tool-call batch carried into continuation requests.
    pub(super) tool_rounds: Vec<ToolRound>,
    /// This stream's serviced calls, batched into `tool_rounds` when the
    /// stream ends without a terminal; non-empty means the current stream
    /// still owes a continuation.
    pub(super) current_calls: Vec<CommittedToolCall>,
    /// Assistant text emitted by the current stream, retained with its Tool
    /// round when the stream requires continuation.
    pub(super) current_assistant_content: String,
    /// Application-owned accumulator coalescing raw provider fragments into
    /// bounded durable deltas (ADR-0005 PLB-1/PLB-2).
    pub(super) delta_coalescer: DeltaCoalescer,
}

impl ExecutionState {
    fn started() -> Self {
        Self {
            published: Vec::new(),
            observed_len: 0,
            usage: Usage::zero(),
            lifecycle: Turn::start(),
            provider_item_count: 0,
            provider_payload_bytes: 0,
            tool_rounds: Vec::new(),
            current_calls: Vec::new(),
            current_assistant_content: String::new(),
            delta_coalescer: DeltaCoalescer::empty(),
        }
    }
}

impl<P, H> TurnRunner<P, H, NoToolExecution>
where
    P: ModelProvider,
    H: TurnHistory,
{
    /// Creates a runner from consumer-owned provider and history ports.
    ///
    /// Model Tool calls fail closed with a recorded typed unavailability
    /// until [`Self::with_tool_executor`] assembles a C-5 boundary.
    #[must_use]
    pub const fn new(provider: P, history: H) -> Self {
        Self {
            provider,
            history,
            tools: NoToolExecution,
        }
    }
}

impl<P, H, T> TurnRunner<P, H, T>
where
    P: ModelProvider,
    H: TurnHistory,
    T: ToolCallExecutor,
{
    /// Returns a runner whose model Tool calls are serviced through the
    /// supplied C-5 tool-execution boundary.
    #[must_use]
    pub fn with_tool_executor<E: ToolCallExecutor>(self, tools: E) -> TurnRunner<P, H, E> {
        TurnRunner {
            provider: self.provider,
            history: self.history,
            tools,
        }
    }
}

impl<P, H, T> TurnRunner<P, H, T>
where
    P: ModelProvider,
    H: TurnHistory,
    T: ToolCallExecutor,
{
    /// Cancels live Tool work and records the canonical interrupt terminal.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError::Tool`] when a live D-7 cannot be terminalized,
    /// and [`TurnRunError::History`] when the Turn is unknown, non-owned,
    /// already terminal, fenced, or the durable store is unavailable.
    pub fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), TurnRunError> {
        let thread_id = self.history.interruption_thread(trust, turn_id)?;
        if let Some(thread_id) = thread_id {
            let tool_terminals = self.tools.request_interrupt(trust, thread_id, turn_id)?;
            // History owns the atomic order: every C-5 D-7 terminal precedes
            // the Turn terminal, so replay and SSE never strand a running view.
            let interrupt_result = self
                .history
                .request_interrupt(trust, turn_id, tool_terminals);
            self.notify_terminal(trust, thread_id, turn_id);
            interrupt_result?;
            return Ok(());
        }
        let interrupt_result = self.history.request_interrupt(trust, turn_id, Vec::new());
        // A lost acknowledgement or competing terminal may have committed the
        // durable terminal even when `request_interrupt` reports an error.
        // The boundary's probe decides whether local authority can release.
        if let Some(thread_id) = thread_id {
            self.notify_terminal(trust, thread_id, turn_id);
        }
        interrupt_result?;
        Ok(())
    }

    /// Notifies the tool boundary after a durable Turn terminal so its
    /// fail-closed probe can reclaim process-local authority (ADR-0003 T-3).
    fn notify_terminal(&mut self, trust: &TrustContext, thread_id: ThreadId, turn_id: TurnId) {
        self.tools
            .turn_terminal_committed(&trust.tenant_id, thread_id, turn_id);
    }

    /// Executes one accepted turn and publishes only successfully appended items.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] when initial acceptance, provider setup, append,
    /// replay, or an internal lifecycle transition fails.
    pub fn execute(&mut self, command: TurnCommand) -> Result<TurnResult, TurnRunError> {
        self.execute_with_observer(command, &mut |_| {})
    }

    /// Executes one turn while observing only durably committed stream events.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] under the same conditions as [`Self::execute`].
    pub fn execute_with_observer(
        &mut self,
        command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, TurnRunError> {
        self.execute_with_observer_and_cancellation(command, observer, &|| false)
    }

    /// Executes one observed turn and durably cancels it when its consumer disconnects.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] under the same conditions as [`Self::execute`].
    pub fn execute_with_observer_and_cancellation(
        &mut self,
        command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnResult, TurnRunError> {
        let prior_history = command
            .thread_id
            .map(|thread_id| self.history.prior_thread_items(&command.trust, thread_id))
            .transpose()
            .map_err(|error| history_failure(error, false, &[]))?
            .unwrap_or_default();
        let accepted = self
            .history
            .accept_initial(&command)
            .map_err(|error| history_failure(error, false, &[]))?;
        let liveness = match self.history.start_turn_liveness(&accepted) {
            Ok(liveness) => liveness,
            Err(error) => {
                let close = self.history.append_provider_terminal(
                    &accepted,
                    TerminalOutcome::Failed {
                        code: "DURABILITY_UNAVAILABLE".to_owned(),
                    },
                );
                if close == Err(HistoryError::Unavailable) {
                    let _ = self.history.schedule_failed_recovery(&accepted);
                }
                return Err(history_failure(error, true, &[]));
            }
        };
        observer(TurnStreamEvent::Started {
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
        });
        let input = ModelInput {
            tenant_id: command.trust.tenant_id.clone(),
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
            input: command.input,
            history: prior_history,
            tool_rounds: Vec::new(),
        };
        let mut state = ExecutionState::started();
        match run_accepted(
            &mut self.provider,
            &mut self.history,
            &mut self.tools,
            &accepted,
            &command.trust,
            &mut state,
            input,
            observer,
            cancelled,
        ) {
            // The durable Turn terminal may release the boundary's Turn authority (ADR-0003 T-3).
            Ok(()) => {
                self.notify_terminal(&command.trust, accepted.thread_id, accepted.turn_id);
            }
            Err(TurnRunError::Durability(failure)) => {
                let mut terminal_notified = false;
                if state.lifecycle.status() == crate::domain::TurnStatus::RecoveryPending {
                    let handoff = liveness.handoff_to_recovery()?;
                    if handoff == super::RecoveryHandoff::Released
                        && let Err(schedule_error) =
                            self.history.schedule_failed_recovery(&accepted)
                        && schedule_error != HistoryError::Unavailable
                    {
                        return Err(TurnRunError::History(schedule_error));
                    }
                    if handoff == super::RecoveryHandoff::Recovered {
                        // A recovered handoff has already committed the
                        // canonical terminal. Notify C-5 before replay so
                        // its fail-closed probe can reclaim process-local
                        // authority even if replay becomes unavailable.
                        self.notify_terminal(&command.trust, accepted.thread_id, accepted.turn_id);
                        terminal_notified = true;
                        if let Err(error) = publish_replayed_terminal(
                            &self.history,
                            &accepted,
                            &mut state,
                            observer,
                        ) && !matches!(
                            error,
                            TurnRunError::History(HistoryError::Fenced | HistoryError::NotFound)
                                | TurnRunError::Durability(_)
                        ) {
                            return Err(error);
                        }
                    }
                }
                if !terminal_notified {
                    // A durability failure may still follow a committed
                    // durable terminal — terminalize_from_limit closes the
                    // Turn as Failed(DURABILITY_UNAVAILABLE) before returning
                    // here — so notify fail-closed: the probe independently
                    // proves the terminal and safely retains authority when
                    // none is provable (ADR-0003 T-3).
                    self.notify_terminal(&command.trust, accepted.thread_id, accepted.turn_id);
                }
                return Err(TurnRunError::Durability(failure));
            }
            Err(TurnRunError::ResourceLimit(failure)) => {
                // The durable `RESOURCE_LIMIT_EXCEEDED` terminal committed
                // before this failure surfaced, so the boundary's fail-closed
                // probe can reclaim its process-local authority (ADR-0003
                // T-3, ADR-0005 PLB-7).
                self.notify_terminal(&command.trust, accepted.thread_id, accepted.turn_id);
                return Err(TurnRunError::ResourceLimit(failure));
            }
            Err(error) => return Err(error),
        }
        Self::finish(
            &self.history,
            &command.trust.tenant_id,
            &accepted,
            state.lifecycle,
            state.published,
        )
    }

    fn finish(
        history: &H,
        tenant_id: &TenantId,
        accepted: &AcceptedTurn,
        lifecycle: Turn,
        published: Vec<Item>,
    ) -> Result<TurnResult, TurnRunError> {
        let replay = history
            .replay(tenant_id, accepted.turn_id)
            .map_err(|error| history_failure(error, true, &published))?;
        Ok(TurnResult {
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
            status: lifecycle.status(),
            published,
            replay,
        })
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one independently validated orchestration input"
)]
fn run_accepted<P: ModelProvider, H: TurnHistory, T: ToolCallExecutor>(
    provider: &mut P,
    history: &mut H,
    tools: &mut T,
    accepted: &AcceptedTurn,
    trust: &crate::domain::TrustContext,
    state: &mut ExecutionState,
    input: ModelInput,
    observer: &mut dyn FnMut(TurnStreamEvent),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), TurnRunError> {
    let mut input = input;
    loop {
        let mut stream = match provider.stream(input.clone()) {
            Ok(stream) => stream,
            Err(error) => {
                append_terminal_or_replay_fenced(
                    history,
                    accepted,
                    state,
                    TerminalOutcome::Failed { code: error.code },
                    observer,
                )?;
                return Ok(());
            }
        };
        let reached_terminal = drive_stream(
            history,
            tools,
            accepted,
            trust,
            state,
            &mut *stream,
            observer,
            cancelled,
        )?;
        drop(stream);
        if reached_terminal {
            return Ok(());
        }
        if !state.current_calls.is_empty() {
            // The provider finished a Tool-call round without a terminal:
            // batch the round and start the continuation request carrying
            // every bounded committed result in causal order (ADR-0003
            // TC-11). Completion is accepted only from a continuation stream.
            state.tool_rounds.push(ToolRound {
                assistant_content: std::mem::take(&mut state.current_assistant_content),
                calls: std::mem::take(&mut state.current_calls),
            });
            input.tool_rounds.clone_from(&state.tool_rounds);
            continue;
        }
        append_terminal_or_replay_fenced(
            history,
            accepted,
            state,
            TerminalOutcome::Failed {
                code: "PROVIDER_STREAM_ENDED".to_owned(),
            },
            observer,
        )?;
        return Ok(());
    }
}

fn append_terminal_or_replay_fenced<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<(), TurnRunError> {
    match append_provider_terminal_observed(history, accepted, state, outcome, observer) {
        Err(TurnRunError::History(HistoryError::Fenced | HistoryError::AlreadyTerminal)) => {
            publish_replayed_terminal(history, accepted, state, observer)
        }
        result => result,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one independently validated orchestration input"
)]
fn drive_stream<H: TurnHistory, T: ToolCallExecutor>(
    history: &mut H,
    tools: &mut T,
    accepted: &AcceptedTurn,
    trust: &crate::domain::TrustContext,
    state: &mut ExecutionState,
    stream: &mut dyn Iterator<Item = ProviderEvent>,
    observer: &mut dyn FnMut(TurnStreamEvent),
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, TurnRunError> {
    let mut last_interruption_poll = None;
    for event in stream {
        // The latency deadline is sampled and flushed before any potentially
        // blocking control read and before every event — including a backlog
        // of consecutive Delta frames that never reaches the Pending arm — so
        // a due buffered chunk publishes no later than 500 ms after its first
        // byte, with its outcome arbitrated like any event failure
        // (ADR-0005 PLB-2/PLB-7).
        if let Some(due) = state.delta_coalescer.take_due_flush(Instant::now()) {
            let outcome = append_coalesced_deltas(history, accepted, state, [due], observer);
            if arbitrate_flush_outcome(history, accepted, state, outcome, observer)? {
                return Ok(true);
            }
        }
        let interruption_poll_due = last_interruption_poll
            .is_none_or(|last: Instant| last.elapsed() >= INTERRUPTION_POLL_INTERVAL);
        if interruption_poll_due {
            last_interruption_poll = Some(Instant::now());
        }
        // Persisted interruption is polled at most once per bounded window;
        // the durable append paths still arbitrate any interrupt that commits
        // between polls. Consumer cancellation remains an in-process check on
        // every iteration and performs no database query.
        if interruption_poll_due
            && terminalize_from_persisted_interruption(history, accepted, state, observer)?
        {
            return Ok(true);
        }
        if terminalize_from_cancellation(history, accepted, state, observer, cancelled)? {
            return Ok(true);
        }
        let event_result = handle_event(history, tools, accepted, trust, state, event, observer);
        // Observe every item published by the event that was not already
        // observed live at its publish boundary (e.g. tool projections).
        for item in &state.published[state.observed_len..] {
            observe_item(observer, accepted, item);
        }
        state.observed_len = state.published.len();
        if event_terminal_or_recover(history, accepted, state, event_result, observer)? {
            return Ok(true);
        }
    }
    // The stream ended without a terminal: buffered text flushes durably
    // before any Tool-round continuation or stream-ended terminal takes
    // effect (ADR-0005 PLB-3), with the flush outcome arbitrated like any
    // event failure.
    if flush_and_arbitrate(history, accepted, state, observer)? {
        return Ok(true);
    }
    Ok(false)
}

/// Flushes buffered deltas outside the driving loop's observation window and
/// arbitrates the outcome through [`arbitrate_flush_outcome`].
fn flush_and_arbitrate<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    let outcome = flush_buffered_deltas(history, accepted, state, observer);
    arbitrate_flush_outcome(history, accepted, state, outcome, observer)
}

/// Arbitrates one flush outcome exactly like an event failure: any durable
/// terminal the flush appended is observed before the error surfaces — a
/// started stream sees the exact `turn.failed` terminal and never a
/// contradictory error event — a competing writer's terminal is adopted
/// through replay, and a history outage enters the bounded recovery path
/// (ADR-0005 PLB-3/PLB-4/PLB-7).
fn arbitrate_flush_outcome<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: Result<bool, TurnRunError>,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    for item in &state.published[state.observed_len..] {
        observe_item(observer, accepted, item);
    }
    state.observed_len = state.published.len();
    match outcome {
        Ok(closed) => Ok(closed),
        Err(TurnRunError::History(HistoryError::Fenced | HistoryError::AlreadyTerminal)) => {
            publish_replayed_terminal(history, accepted, state, observer)?;
            Ok(true)
        }
        Err(TurnRunError::History(error)) => Err(recover_append_failure(state, error)),
        Err(error) => Err(error),
    }
}

fn terminalize_from_persisted_interruption<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    match history.interruption_requested(accepted) {
        Ok(true) => {
            // Buffered text flushes durably before the winning terminal
            // (ADR-0005 PLB-3), with the flush outcome arbitrated like any
            // event failure.
            if flush_and_arbitrate(history, accepted, state, observer)? {
                return Ok(true);
            }
            if let Err(error) = append_terminal(
                history,
                accepted,
                state,
                TerminalOutcome::Interrupted,
                observer,
            ) {
                if matches!(
                    error,
                    TurnRunError::History(HistoryError::Fenced | HistoryError::AlreadyTerminal)
                ) {
                    publish_replayed_terminal(history, accepted, state, observer)?;
                    return Ok(true);
                }
                return Err(error);
            }
            state.lifecycle = state.lifecycle.interrupt()?;
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(HistoryError::Fenced | HistoryError::AlreadyTerminal) => {
            publish_replayed_terminal(history, accepted, state, observer)?;
            Ok(true)
        }
        Err(error) => Err(recover_append_failure(state, error)),
    }
}

fn terminalize_from_cancellation<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, TurnRunError> {
    if !cancelled() {
        return Ok(false);
    }
    // Buffered text flushes durably before the winning terminal; the same
    // signal serves dependency and downstream-disconnect cancellation
    // (ADR-0005 PLB-3), with the flush outcome arbitrated like any event
    // failure.
    if flush_and_arbitrate(history, accepted, state, observer)? {
        return Ok(true);
    }
    append_terminal_or_replay_fenced(
        history,
        accepted,
        state,
        TerminalOutcome::Cancelled,
        observer,
    )?;
    Ok(true)
}

fn handle_event<H: TurnHistory, T: ToolCallExecutor>(
    history: &mut H,
    tools: &mut T,
    accepted: &AcceptedTurn,
    trust: &crate::domain::TrustContext,
    state: &mut ExecutionState,
    event: ProviderEvent,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    match event {
        ProviderEvent::Delta(content) => {
            // Raw fragments are not canonical Items: the bytes join the
            // application-owned accumulator and only its flushed chunks are
            // accounted and appended (ADR-0005 PLB-1/PLB-2).
            state.current_assistant_content.push_str(&content);
            let chunks = state.delta_coalescer.push(&content, Instant::now());
            append_coalesced_deltas(history, accepted, state, chunks, observer)
        }
        ProviderEvent::Usage(counters) => {
            // Every request of one Turn — including each Tool-call
            // continuation — reports its own counters; the Turn terminal
            // carries their checked sum (ADR-0003 TC-11). Buffered text
            // flushes durably before the usage boundary (ADR-0005 PLB-3).
            if flush_buffered_deltas(history, accepted, state, observer)? {
                return Ok(true);
            }
            if let Ok(total) = state.usage.checked_accumulate(&counters) {
                state.usage = total;
            } else {
                append_provider_terminal(
                    history,
                    accepted,
                    state,
                    TerminalOutcome::Failed {
                        code: "PROVIDER_USAGE_OVERFLOW".to_owned(),
                    },
                )?;
                return Ok(true);
            }
            let item = NewItem::Usage(counters);
            if enforce_provider_limit(history, accepted, state, &item)? {
                return Ok(true);
            }
            let durable = history.append(accepted, item)?;
            accept_appended_provider_item(state, durable, false)
        }
        ProviderEvent::Completed => {
            // Buffered text flushes durably before the completion boundary
            // (ADR-0005 PLB-3).
            if flush_buffered_deltas(history, accepted, state, observer)? {
                return Ok(true);
            }
            if !state.current_calls.is_empty() {
                // A completion on a stream that still owes Tool-call
                // continuation is a provider protocol violation: the committed
                // results never reached the model. Fail closed instead of
                // closing the Turn (ADR-0003 TC-11).
                append_provider_terminal(
                    history,
                    accepted,
                    state,
                    TerminalOutcome::Failed {
                        code: "PROVIDER_PREMATURE_COMPLETION".to_owned(),
                    },
                )?;
                return Ok(true);
            }
            let item = NewItem::Terminal(TerminalOutcome::Completed { usage: state.usage });
            if enforce_provider_limit(history, accepted, state, &item)? {
                return Ok(true);
            }
            append_provider_terminal(
                history,
                accepted,
                state,
                TerminalOutcome::Completed { usage: state.usage },
            )?;
            Ok(true)
        }
        ProviderEvent::Error { code } => {
            // Buffered text flushes durably before the provider error
            // terminal (ADR-0005 PLB-3).
            if flush_buffered_deltas(history, accepted, state, observer)? {
                return Ok(true);
            }
            let outcome = TerminalOutcome::Failed { code };
            let item = NewItem::Terminal(outcome.clone());
            if enforce_provider_limit(history, accepted, state, &item)? {
                return Ok(true);
            }
            append_provider_terminal(history, accepted, state, outcome)?;
            Ok(true)
        }
        ProviderEvent::ToolCall { name, arguments } => {
            // Buffered text flushes durably before Tool-call delivery
            // (ADR-0005 PLB-3).
            if flush_buffered_deltas(history, accepted, state, observer)? {
                return Ok(true);
            }
            tool_call::handle_tool_call(
                history, tools, accepted, trust, state, name, arguments, observer,
            )
        }
        ProviderEvent::Pending => Ok(false),
    }
}
