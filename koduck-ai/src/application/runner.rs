// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Provider-neutral lifecycle orchestration and durable-before-visible ordering.

use crate::domain::{Item, TenantId, TerminalOutcome, ThreadId, TrustContext, Turn, TurnId, Usage};

use super::delta_coalescer::DeltaCoalescer;
use super::ports::{
    AcceptedTurn, CommittedToolCall, DurabilityFailure, HistoryError, ModelInput, ModelProvider,
    NoToolExecution, ToolCallExecutor, ToolRound, TurnCommand, TurnHistory, TurnLiveness,
    TurnResult, TurnRunError, TurnStreamEvent,
};

pub(super) mod failure;
pub(super) mod runner_stream;
pub(super) mod tool_call;

use failure::history_failure;

use runner_stream::{append_terminal_or_replay_fenced, drive_stream};

use super::runner_terminals::publish_replayed_terminal;

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
        let result = run_accepted(
            &mut self.provider,
            &mut self.history,
            &mut self.tools,
            &accepted,
            &command.trust,
            &mut state,
            input,
            observer,
            cancelled,
        );
        match result {
            // The durable Turn terminal may release the boundary's Turn authority (ADR-0003 T-3).
            Ok(()) => {
                self.notify_terminal(&command.trust, accepted.thread_id, accepted.turn_id);
            }
            Err(TurnRunError::Durability(failure)) => {
                return self.settle_durability_failure(
                    &command.trust,
                    &accepted,
                    liveness,
                    &mut state,
                    observer,
                    failure,
                );
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

    /// Runs the bounded recovery handoff after a durability failure and
    /// surfaces the failure. A recovery-pending Turn hands its liveness into
    /// recovery; a recovered handoff has already committed the canonical
    /// terminal, so it is notified before replay. Every path notifies the
    /// durable terminal fail-closed: the probe independently proves the
    /// terminal and safely retains authority when none is provable
    /// (ADR-0003 T-3).
    fn settle_durability_failure(
        &mut self,
        trust: &TrustContext,
        accepted: &AcceptedTurn,
        liveness: Box<dyn TurnLiveness>,
        state: &mut ExecutionState,
        observer: &mut dyn FnMut(TurnStreamEvent),
        failure: DurabilityFailure,
    ) -> Result<TurnResult, TurnRunError> {
        let mut terminal_notified = false;
        if state.lifecycle.status() == crate::domain::TurnStatus::RecoveryPending {
            let handoff = liveness.handoff_to_recovery()?;
            if handoff == super::RecoveryHandoff::Released
                && let Err(schedule_error) = self.history.schedule_failed_recovery(accepted)
                && schedule_error != HistoryError::Unavailable
            {
                return Err(TurnRunError::History(schedule_error));
            }
            if handoff == super::RecoveryHandoff::Recovered {
                // A recovered handoff has already committed the
                // canonical terminal. Notify C-5 before replay so
                // its fail-closed probe can reclaim process-local
                // authority even if replay becomes unavailable.
                self.notify_terminal(trust, accepted.thread_id, accepted.turn_id);
                terminal_notified = true;
                if let Err(error) =
                    publish_replayed_terminal(&self.history, accepted, state, observer)
                    && !matches!(
                        error,
                        TurnRunError::History(HistoryError::Fenced | HistoryError::NotFound)
                            | TurnRunError::Durability(_)
                    )
                {
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
            self.notify_terminal(trust, accepted.thread_id, accepted.turn_id);
        }
        Err(TurnRunError::Durability(failure))
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
