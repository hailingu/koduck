// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Provider-neutral lifecycle orchestration and durable-before-visible ordering.

use crate::domain::{Item, TenantId, TerminalOutcome, TrustContext, Turn, TurnId, Usage};

use super::AppendPolicy;
use super::ports::{
    AcceptedTurn, DurabilityFailure, HistoryError, ModelInput, ModelProvider, NewItem,
    ProviderEvent, TurnCommand, TurnHistory, TurnResult, TurnRunError, TurnStreamEvent,
};

/// Owns provider-neutral lifecycle transitions and durable-before-visible ordering.
#[derive(Clone)]
pub struct TurnRunner<P, H> {
    provider: P,
    history: H,
}

struct ExecutionState {
    published: Vec<Item>,
    usage: Usage,
    lifecycle: Turn,
    provider_item_count: usize,
}

impl ExecutionState {
    fn started() -> Self {
        Self {
            published: Vec::new(),
            usage: Usage::zero(),
            lifecycle: Turn::start(),
            provider_item_count: 0,
        }
    }
}

impl<P, H> TurnRunner<P, H>
where
    P: ModelProvider,
    H: TurnHistory,
{
    /// Creates a runner from consumer-owned provider and history ports.
    #[must_use]
    pub const fn new(provider: P, history: H) -> Self {
        Self { provider, history }
    }

    /// Records an interrupt request through the canonical history boundary.
    ///
    /// # Errors
    ///
    /// Returns [`TurnRunError::History`] when the turn is unknown, non-owned,
    /// already terminal, fenced, or the durable store is unavailable.
    pub fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), TurnRunError> {
        self.history.request_interrupt(trust, turn_id)?;
        Ok(())
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
        let _liveness = match self.history.start_turn_liveness(&accepted) {
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
        };
        let state = run_accepted(
            &mut self.provider,
            &mut self.history,
            &accepted,
            input,
            observer,
        )?;
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

fn run_accepted<P: ModelProvider, H: TurnHistory>(
    provider: &mut P,
    history: &mut H,
    accepted: &AcceptedTurn,
    input: ModelInput,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<ExecutionState, TurnRunError> {
    let mut state = ExecutionState::started();
    let mut stream = match provider.stream(input) {
        Ok(stream) => stream,
        Err(error) => {
            append_provider_terminal_observed(
                history,
                accepted,
                &mut state,
                TerminalOutcome::Failed { code: error.code },
                observer,
            )?;
            return Ok(state);
        }
    };
    let reached_terminal = drive_stream(history, accepted, &mut state, &mut *stream, observer)?;
    drop(stream);
    if !reached_terminal {
        append_provider_terminal_observed(
            history,
            accepted,
            &mut state,
            TerminalOutcome::Failed {
                code: "PROVIDER_STREAM_ENDED".to_owned(),
            },
            observer,
        )?;
    }
    Ok(state)
}

fn drive_stream<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    stream: &mut dyn Iterator<Item = ProviderEvent>,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    for event in stream {
        let published_before = state.published.len();
        let event_result = handle_event(history, accepted, state, event);
        for item in &state.published[published_before..] {
            observe_item(observer, accepted, item);
        }
        if event_terminal_or_recover(history, accepted, state, event_result)? {
            return Ok(true);
        }
        match history.interruption_requested(accepted) {
            Ok(true) => {
                append_terminal(
                    history,
                    accepted,
                    state,
                    TerminalOutcome::Interrupted,
                    observer,
                )?;
                state.lifecycle = state.lifecycle.interrupt()?;
                return Ok(true);
            }
            Ok(false) => {}
            Err(HistoryError::Fenced) => {
                publish_replayed_terminal(history, accepted, state, observer)?;
                return Ok(true);
            }
            Err(error) => return Err(history_failure(error, true, &state.published)),
        }
    }
    Ok(false)
}

fn publish_replayed_terminal<H: TurnHistory>(
    history: &H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<(), TurnRunError> {
    let replay = history
        .replay(&accepted.tenant_id, accepted.turn_id)
        .map_err(|error| history_failure(error, true, &state.published))?;
    let terminal = replay
        .last()
        .filter(|item| matches!(item.payload, crate::domain::ItemPayload::Terminal(_)))
        .ok_or(HistoryError::Fenced)?
        .clone();
    let crate::domain::ItemPayload::Terminal(outcome) = &terminal.payload else {
        return Err(HistoryError::Fenced.into());
    };
    apply_terminal_outcome(state, outcome)?;
    observe_item(observer, accepted, &terminal);
    state.published.push(terminal);
    Ok(())
}

fn event_terminal_or_recover<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    event_result: Result<bool, TurnRunError>,
) -> Result<bool, TurnRunError> {
    match event_result {
        Ok(terminal) => Ok(terminal),
        Err(TurnRunError::History(error)) => {
            Err(recover_append_failure(history, accepted, state, error))
        }
        Err(error) => Err(post_accept_failure(error, &state.published)),
    }
}

fn append_terminal<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<(), TurnRunError> {
    let terminal = history
        .append(accepted, NewItem::Terminal(outcome))
        .map_err(|error| recover_append_failure(history, accepted, state, error))?;
    observe_item(observer, accepted, &terminal);
    state.published.push(terminal);
    Ok(())
}

fn append_provider_terminal_observed<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<(), TurnRunError> {
    append_provider_terminal(history, accepted, state, outcome)?;
    let terminal = state.published.last().ok_or(HistoryError::Unavailable)?;
    observe_item(observer, accepted, terminal);
    Ok(())
}

fn append_provider_terminal<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
) -> Result<(), TurnRunError> {
    let terminal = history
        .append_provider_terminal(accepted, outcome)
        .map_err(|error| recover_append_failure(history, accepted, state, error))?;
    let crate::domain::ItemPayload::Terminal(actual) = &terminal.payload else {
        return Err(HistoryError::Unavailable.into());
    };
    apply_terminal_outcome(state, actual)?;
    state.published.push(terminal);
    Ok(())
}

fn observe_item(observer: &mut dyn FnMut(TurnStreamEvent), accepted: &AcceptedTurn, item: &Item) {
    observer(TurnStreamEvent::Item {
        thread_id: accepted.thread_id,
        turn_id: accepted.turn_id,
        item: item.clone(),
    });
}

fn handle_event<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    event: ProviderEvent,
) -> Result<bool, TurnRunError> {
    match event {
        ProviderEvent::Delta(content) => {
            let item = NewItem::AgentMessageDelta { content };
            validate_provider_item(state, &item)?;
            let durable = history.append(accepted, item)?;
            accept_appended_provider_item(state, durable, true)
        }
        ProviderEvent::Usage(observed) => {
            state.usage = observed;
            let item = NewItem::Usage(observed);
            validate_provider_item(state, &item)?;
            let durable = history.append(accepted, item)?;
            accept_appended_provider_item(state, durable, false)
        }
        ProviderEvent::Completed => {
            let item = NewItem::Terminal(TerminalOutcome::Completed { usage: state.usage });
            validate_provider_item(state, &item)?;
            append_provider_terminal(
                history,
                accepted,
                state,
                TerminalOutcome::Completed { usage: state.usage },
            )?;
            Ok(true)
        }
        ProviderEvent::Error { code } => {
            let outcome = TerminalOutcome::Failed { code };
            let item = NewItem::Terminal(outcome.clone());
            validate_provider_item(state, &item)?;
            append_provider_terminal(history, accepted, state, outcome)?;
            Ok(true)
        }
        ProviderEvent::Pending => Ok(false),
    }
}

fn validate_provider_item(state: &mut ExecutionState, item: &NewItem) -> Result<(), HistoryError> {
    let policy = AppendPolicy::cand_1();
    let next_count = state.provider_item_count.saturating_add(1);
    policy
        .check_item_count(next_count)
        .and_then(|()| policy.check_item(item))
        .map_err(|_| HistoryError::Unavailable)?;
    state.provider_item_count = next_count;
    Ok(())
}

fn accept_appended_provider_item(
    state: &mut ExecutionState,
    item: Item,
    publish_nonterminal: bool,
) -> Result<bool, TurnRunError> {
    if let crate::domain::ItemPayload::Terminal(actual) = &item.payload {
        apply_terminal_outcome(state, actual)?;
        state.published.push(item);
        return Ok(true);
    }
    if publish_nonterminal {
        state.published.push(item);
    }
    Ok(false)
}

fn apply_terminal_outcome(
    state: &mut ExecutionState,
    outcome: &TerminalOutcome,
) -> Result<(), TurnRunError> {
    state.lifecycle = match outcome {
        TerminalOutcome::Completed { .. } => state.lifecycle.complete()?,
        TerminalOutcome::Failed { .. } => state.lifecycle.fail()?,
        TerminalOutcome::Interrupted => state.lifecycle.interrupt()?,
        TerminalOutcome::Cancelled => state.lifecycle.cancel()?,
    };
    Ok(())
}

fn history_failure(error: HistoryError, accepted: bool, published: &[Item]) -> TurnRunError {
    if error == HistoryError::Unavailable {
        TurnRunError::Durability(DurabilityFailure {
            accepted,
            published: published.to_vec(),
            source: error,
        })
    } else {
        TurnRunError::History(error)
    }
}

fn post_accept_failure(error: TurnRunError, published: &[Item]) -> TurnRunError {
    match error {
        TurnRunError::History(history_error) => history_failure(history_error, true, published),
        other => other,
    }
}

fn recover_append_failure<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    error: HistoryError,
) -> TurnRunError {
    if error == HistoryError::Unavailable {
        if let Ok(recovery_pending) = state.lifecycle.recovery_pending() {
            state.lifecycle = recovery_pending;
            if let Err(schedule_error) = history.schedule_failed_recovery(accepted) {
                if schedule_error != HistoryError::Unavailable {
                    return TurnRunError::History(schedule_error);
                }
            }
        }
    }
    history_failure(error, true, &state.published)
}
