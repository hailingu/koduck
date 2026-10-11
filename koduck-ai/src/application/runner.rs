// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md
// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md
// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! Provider-neutral lifecycle orchestration and durable-before-visible ordering.

use std::sync::Arc;

use crate::domain::{Item, TenantId, TerminalOutcome, ThreadId, TrustContext, Turn, TurnId, Usage};

use super::delta_coalescer::DeltaCoalescer;
use super::ports::{
    AcceptedTurn, CommittedToolCall, DurabilityFailure, HistoryError, ModelInput, ModelProvider,
    NoToolExecution, ToolCallExecutor, ToolRound, TurnCommand, TurnHistory, TurnLiveness,
    TurnResult, TurnRunError, TurnStreamEvent,
};
use super::provider_context::{ProviderHistoryItem, prepare_provider_history};
use super::submission::{
    AcceptanceBudget, AcceptanceClock, IdentifiedAcceptance, LOOKUP_BUDGET, TurnOutcome,
    WRITE_BUDGET, WRITE_GATE_MINIMUM, system_clock, validate_identified_command,
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
    acceptance_clock: Arc<dyn AcceptanceClock>,
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
    /// Creates a runner from consumer-owned provider and history ports with
    /// the production system acceptance clock.
    ///
    /// Model Tool calls fail closed with a recorded typed unavailability
    /// until [`Self::with_tool_executor`] assembles a C-5 boundary.
    #[must_use]
    pub fn new(provider: P, history: H) -> Self {
        Self {
            provider,
            history,
            tools: NoToolExecution,
            acceptance_clock: system_clock(),
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
            acceptance_clock: self.acceptance_clock,
        }
    }

    /// Returns a runner whose identified-submission acceptance policy reads
    /// the supplied monotonic acceptance clock (ADR-0018 SI-07i).
    ///
    /// The clock dependency is retained through cloning and Tool
    /// composition, while every identified request derives its own separate
    /// start reading and budget. The production runtime supplies the system
    /// clock explicitly through this builder; tests may substitute a manual
    /// clock without altering any real database, lease, or fenced-recovery
    /// timer.
    #[must_use]
    pub fn with_acceptance_clock(mut self, acceptance_clock: Arc<dyn AcceptanceClock>) -> Self {
        self.acceptance_clock = acceptance_clock;
        self
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
        // SI-01d: the legacy entries return one owned TurnResult and cannot
        // carry submission semantics; an identified command is rejected here
        // instead of silently executing through the unidentified path.
        if command.submission_id.is_some() {
            return Err(TurnRunError::InvalidCommand(
                super::ports::TurnCommandError::SubmissionOnLegacyEntry,
            ));
        }
        // Prepare the effective provider view from one ordered read snapshot
        // before any acceptance, and reuse it unchanged for this Turn's
        // continuations (ADR-0006 PC-02).
        let prepared_history = match command.thread_id {
            Some(thread_id) => {
                Self::prepare_prior_history(&self.history, &command.trust, thread_id)?
            }
            None => Vec::new(),
        };
        let accepted = self
            .history
            .accept_initial(&command)
            .map_err(|error| history_failure(error, false, &[]))?;
        self.run_created_turn(
            &command.trust,
            command.input,
            &accepted,
            prepared_history,
            observer,
            cancelled,
        )
    }

    /// Executes one identified or unidentified chat submission and separates
    /// the created owner from an observed existing key (ADR-0018 SI-04,
    /// SI-07).
    ///
    /// An unidentified command keeps the exact existing acceptance behavior
    /// and returns [`TurnOutcome::Owned`]. An identified command performs the
    /// unlocked owned lookup, fresh prior-history preparation, the
    /// cancellation-first remaining-4-second write gate, and one atomic
    /// acceptance with its full reserved settlement; an exact existing key
    /// returns [`TurnOutcome::Observed`] with no preparation-side execution
    /// authority.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] under the same conditions as
    /// [`Self::execute_with_observer_and_cancellation`], plus
    /// [`TurnRunError::Cancelled`] for an observed pre-acceptance
    /// cancellation (SI-07g) and the typed
    /// [`HistoryError::SubmissionConflict`] for an owned-key drift (SI-02d).
    pub fn execute_submission_with_observer_and_cancellation(
        &mut self,
        command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnOutcome, TurnRunError> {
        if command.submission_id.is_none() {
            // The unidentified path retains the existing acceptance behavior
            // and never writes a binding (SI-03a, SI-10).
            return self
                .execute_with_observer_and_cancellation(command, observer, cancelled)
                .map(TurnOutcome::Owned);
        }
        self.execute_identified_submission(&command, observer, cancelled)
    }

    /// Runs the identified-submission acceptance flow (SI-07a through
    /// SI-07i) on a command that already carries a validated submission
    /// identity.
    fn execute_identified_submission(
        &mut self,
        command: &TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnOutcome, TurnRunError> {
        // SI-01d: the direct-command guard revalidates the owned input byte
        // bound and the non-nil submission identity before any history I/O,
        // so a directly constructed invalid command can never reach the
        // preliminary lookup or the acceptance write.
        validate_identified_command(command)?;
        // SI-07d: the monotonic acceptance deadline starts at fully validated
        // input/trust entry immediately before the preliminary lookup. Each
        // identified request owns this separate budget (SI-07i).
        let budget = AcceptanceBudget::start(self.acceptance_clock.now());
        // SI-07a: the preliminary lookup is read-only and unlocked, bounded
        // by the remaining acceptance time (SI-07c). An unavailable lookup
        // fails closed before any preparation or write.
        let Some(lookup_deadline) = budget.clamp(LOOKUP_BUDGET, self.acceptance_clock.now()) else {
            return Err(TurnRunError::History(HistoryError::Unavailable));
        };
        match self
            .history
            .submission_observation(command, lookup_deadline)
        {
            Ok(Some(receipt)) => return Ok(TurnOutcome::Observed(receipt)),
            Ok(None) => {}
            Err(HistoryError::SubmissionConflict) => {
                return Err(TurnRunError::History(HistoryError::SubmissionConflict));
            }
            Err(error) => return Err(TurnRunError::History(error)),
        }
        self.accept_identified_fresh(command, &budget, observer, cancelled)
    }

    /// Prepares the fresh prior context and runs the cancellation-first
    /// write gate and the atomic acceptance for one absent key (SI-07a
    /// through SI-07d).
    fn accept_identified_fresh(
        &mut self,
        command: &TurnCommand,
        budget: &AcceptanceBudget,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnOutcome, TurnRunError> {
        let prepared_history = match self.identified_prepared_history(command, budget) {
            Ok(history) => history,
            Err(rejection) => {
                return self.resolve_rejected_preparation(command, budget, rejection);
            }
        };
        // SI-07g and SI-07d: cancellation is checked first, then time. An
        // observed pre-acceptance cancellation wins over simultaneous time
        // shortage, and less than four remaining seconds starts no write.
        if cancelled() {
            return Err(TurnRunError::Cancelled);
        }
        let Some(remaining) = budget.remaining(self.acceptance_clock.now()) else {
            return Err(TurnRunError::History(HistoryError::Unavailable));
        };
        if remaining < WRITE_GATE_MINIMUM {
            return Err(TurnRunError::History(HistoryError::Unavailable));
        }
        // SI-03a, SI-06, SI-07c: one atomic acceptance whose write attempt
        // and permitted reconciliation each retain their full reserved
        // budgets inside the port.
        match self
            .history
            .accept_initial_with_submission(command, WRITE_BUDGET)
        {
            Ok(IdentifiedAcceptance::Created(accepted)) => self.finish_created_identified(
                command,
                &accepted,
                prepared_history,
                observer,
                cancelled,
            ),
            Ok(IdentifiedAcceptance::Existing(receipt)) => Ok(TurnOutcome::Observed(receipt)),
            Err(HistoryError::SubmissionConflict) => {
                Err(TurnRunError::History(HistoryError::SubmissionConflict))
            }
            Err(error) => Err(history_failure(error, false, &[])),
        }
    }

    /// Prepares the effective prior view for a fresh identified key with the
    /// history read clamped to the remaining acceptance time (SI-07a,
    /// SI-07c).
    fn identified_prepared_history(
        &self,
        command: &TurnCommand,
        budget: &AcceptanceBudget,
    ) -> Result<Vec<ProviderHistoryItem>, TurnRunError> {
        let Some(thread_id) = command.thread_id else {
            return Ok(Vec::new());
        };
        let Some(deadline) = budget.clamp(LOOKUP_BUDGET, self.acceptance_clock.now()) else {
            return Err(TurnRunError::History(HistoryError::Unavailable));
        };
        let groups = self
            .history
            .prior_thread_turns_bounded(&command.trust, thread_id, deadline)
            .map_err(TurnRunError::History)?;
        Ok(prepare_provider_history(
            &command.trust,
            thread_id,
            &groups,
        )?)
    }

    /// SI-07h/SI-07d: a proven Created owner hands off to the existing
    /// execution flow — or, when its cancellation already fired, to the
    /// durable cancellation path — regardless of later acceptance-clock
    /// expiry.
    fn finish_created_identified(
        &mut self,
        command: &TurnCommand,
        accepted: &crate::application::AcceptedTurn,
        prepared_history: Vec<ProviderHistoryItem>,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnOutcome, TurnRunError> {
        if cancelled() {
            return self
                .cancel_created_turn_without_execution(&command.trust, accepted, observer)
                .map(TurnOutcome::Owned);
        }
        self.run_created_turn(
            &command.trust,
            command.input.clone(),
            accepted,
            prepared_history,
            observer,
            cancelled,
        )
        .map(TurnOutcome::Owned)
    }

    /// SI-07b: preparation rejected, so one further bounded unlocked key
    /// lookup decides between a concurrently committed exact receipt, the
    /// typed drift conflict, the preserved original rejection, and a
    /// fail-closed unavailability. This branch starts no acceptance write.
    fn resolve_rejected_preparation(
        &mut self,
        command: &TurnCommand,
        budget: &AcceptanceBudget,
        rejection: TurnRunError,
    ) -> Result<TurnOutcome, TurnRunError> {
        let Some(final_deadline) = budget.clamp(LOOKUP_BUDGET, self.acceptance_clock.now()) else {
            return Err(TurnRunError::History(HistoryError::Unavailable));
        };
        match self.history.submission_observation(command, final_deadline) {
            Ok(Some(receipt)) => Ok(TurnOutcome::Observed(receipt)),
            Ok(None) => Err(rejection),
            Err(HistoryError::SubmissionConflict) => {
                Err(TurnRunError::History(HistoryError::SubmissionConflict))
            }
            Err(_) => Err(TurnRunError::History(HistoryError::Unavailable)),
        }
    }

    /// SI-07h: a proven Created owner whose cancellation already fired
    /// durably closes the Turn as `cancelled` without provider execution,
    /// keeping its binding and exactly one canonical terminal.
    fn cancel_created_turn_without_execution(
        &mut self,
        trust: &TrustContext,
        accepted: &AcceptedTurn,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, TurnRunError> {
        observer(TurnStreamEvent::Started {
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
        });
        let mut state = ExecutionState::started();
        append_terminal_or_replay_fenced(
            &mut self.history,
            accepted,
            &mut state,
            TerminalOutcome::Cancelled,
            observer,
        )?;
        self.notify_terminal(trust, accepted.thread_id, accepted.turn_id);
        Self::finish(
            &self.history,
            &trust.tenant_id,
            accepted,
            state.lifecycle,
            state.published,
        )
    }

    /// Runs one already accepted Turn through liveness, execution, and
    /// terminal finishing — the shared created-owner path of the
    /// unidentified and identified flows.
    fn run_created_turn(
        &mut self,
        trust: &TrustContext,
        input: String,
        accepted: &AcceptedTurn,
        prepared_history: Vec<ProviderHistoryItem>,
        observer: &mut dyn FnMut(TurnStreamEvent),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnResult, TurnRunError> {
        let liveness = self.start_liveness_or_close(accepted)?;
        observer(TurnStreamEvent::Started {
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
        });
        let input = ModelInput {
            tenant_id: trust.tenant_id.clone(),
            thread_id: accepted.thread_id,
            turn_id: accepted.turn_id,
            input,
            history: prepared_history,
            tool_rounds: Vec::new(),
        };
        let mut state = ExecutionState::started();
        let result = run_accepted(
            &mut self.provider,
            &mut self.history,
            &mut self.tools,
            accepted,
            trust,
            &mut state,
            input,
            observer,
            cancelled,
        );
        match result {
            // The durable Turn terminal may release the boundary's Turn authority (ADR-0003 T-3).
            Ok(()) => {
                self.notify_terminal(trust, accepted.thread_id, accepted.turn_id);
            }
            Err(TurnRunError::Durability(failure)) => {
                return self.settle_durability_failure(
                    trust, accepted, liveness, &mut state, observer, failure,
                );
            }
            Err(TurnRunError::ResourceLimit(failure)) => {
                // The durable `RESOURCE_LIMIT_EXCEEDED` terminal committed
                // before this failure surfaced, so the boundary's fail-closed
                // probe can reclaim its process-local authority (ADR-0003
                // T-3, ADR-0005 PLB-7).
                self.notify_terminal(trust, accepted.thread_id, accepted.turn_id);
                return Err(TurnRunError::ResourceLimit(failure));
            }
            Err(error) => return Err(error),
        }
        Self::finish(
            &self.history,
            &trust.tenant_id,
            accepted,
            state.lifecycle,
            state.published,
        )
    }

    /// Reads and prepares the effective prior-history view for one resume
    /// from a single ordered read snapshot, before any acceptance
    /// (ADR-0006 PC-02).
    ///
    /// # Errors
    ///
    /// Returns the exact [`TurnRunError::History`] cause of the bounded
    /// authenticated read — including `Unavailable` for read deadline expiry
    /// or decode failure (PC-07) — and [`TurnRunError::Context`] when
    /// preparation rejects the source.
    fn prepare_prior_history(
        history: &H,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<ProviderHistoryItem>, TurnRunError> {
        let groups = history
            .prior_thread_turns(trust, thread_id)
            .map_err(TurnRunError::History)?;
        Ok(prepare_provider_history(trust, thread_id, &groups)?)
    }

    /// Starts accepted-Turn liveness, closing the Turn as a durable failure
    /// when liveness maintenance cannot start.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError`] when liveness start fails; the close attempt
    /// itself schedules failed recovery when the terminal append is unavailable.
    fn start_liveness_or_close(
        &mut self,
        accepted: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, TurnRunError> {
        match self.history.start_turn_liveness(accepted) {
            Ok(liveness) => Ok(liveness),
            Err(error) => {
                let close = self.history.append_provider_terminal(
                    accepted,
                    TerminalOutcome::Failed {
                        code: "DURABILITY_UNAVAILABLE".to_owned(),
                    },
                );
                if close == Err(HistoryError::Unavailable) {
                    let _ = self.history.schedule_failed_recovery(accepted);
                }
                Err(history_failure(error, true, &[]))
            }
        }
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
