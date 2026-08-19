// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable Turn-terminal append and replay publication for the runner.
//!
//! Every helper appends one canonical terminal through the history port and
//! publishes only the successfully appended item, or replays the terminal a
//! competing writer already committed. `ExecutionState` ownership stays with
//! the runner; these functions only extend its published view.

use crate::domain::{Item, TerminalOutcome};

use super::ports::{
    AcceptedTurn, HistoryError, NewItem, TurnHistory, TurnRunError, TurnStreamEvent,
};
use super::runner::ExecutionState;
use super::runner::failure::{
    apply_terminal_outcome, history_failure, post_accept_failure, recover_append_failure,
};

/// Publishes the terminal a competing writer already committed.
///
/// Replays the canonical history and adopts its last terminal item when this
/// owner lost the append race (fenced or already terminal): the lifecycle
/// applies that outcome and the observer sees exactly the durable item.
///
/// # Errors
///
/// Returns [`TurnRunError`] when replay is unreadable, holds no terminal, or
/// the terminal cannot drive the lifecycle.
pub(super) fn publish_replayed_terminal<H: TurnHistory>(
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

/// Reports one stream event's terminal outcome, adopting a competing writer's
/// terminal through replay when this owner lost the durable append race.
///
/// # Errors
///
/// Returns [`TurnRunError`] through the accepted-turn failure mapping.
pub(super) fn event_terminal_or_recover<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    event_result: Result<bool, TurnRunError>,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    match event_result {
        Ok(terminal) => Ok(terminal),
        Err(TurnRunError::History(HistoryError::Fenced | HistoryError::AlreadyTerminal)) => {
            publish_replayed_terminal(history, accepted, state, observer)?;
            Ok(true)
        }
        Err(TurnRunError::History(error)) => Err(recover_append_failure(state, error)),
        Err(error) => Err(post_accept_failure(error, &state.published)),
    }
}

/// Appends one sequenced terminal item and publishes only the appended copy.
///
/// # Errors
///
/// Returns [`TurnRunError`] through the append-failure recovery mapping.
pub(super) fn append_terminal<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<(), TurnRunError> {
    let terminal = history
        .append(accepted, NewItem::Terminal(outcome))
        .map_err(|error| recover_append_failure(state, error))?;
    observe_item(observer, accepted, &terminal);
    state.published.push(terminal);
    Ok(())
}

/// Appends one provider-owned terminal and then observes the appended item.
///
/// # Errors
///
/// Returns [`TurnRunError`] when the append fails or no appended terminal
/// remains observable.
pub(super) fn append_provider_terminal_observed<H: TurnHistory>(
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

/// Appends one provider-owned terminal and applies its committed outcome to
/// the lifecycle without an observer callback.
///
/// # Errors
///
/// Returns [`TurnRunError`] when the append fails or the appended item is not
/// a terminal.
pub(super) fn append_provider_terminal<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    outcome: TerminalOutcome,
) -> Result<(), TurnRunError> {
    let terminal = history
        .append_provider_terminal(accepted, outcome)
        .map_err(|error| recover_append_failure(state, error))?;
    let crate::domain::ItemPayload::Terminal(actual) = &terminal.payload else {
        return Err(HistoryError::Unavailable.into());
    };
    apply_terminal_outcome(state, actual)?;
    state.published.push(terminal);
    Ok(())
}

/// Delivers one already-durable item to the stream observer with its Turn
/// identity, so publication never precedes its durable append.
pub(super) fn observe_item(
    observer: &mut dyn FnMut(TurnStreamEvent),
    accepted: &AcceptedTurn,
    item: &Item,
) {
    observer(TurnStreamEvent::Item {
        thread_id: accepted.thread_id,
        turn_id: accepted.turn_id,
        item: item.clone(),
    });
}
