// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Provider-item accounting and durable-failure conversion for one Turn.

use crate::domain::{Item, TerminalOutcome};

use super::super::{
    AppendPolicy, BufferLimitError, DurabilityFailure, HistoryError, NewItem, TurnRunError,
};
use super::ExecutionState;

/// Accounts one provider item against the Turn's exact shared buffer budget.
pub(super) fn validate_provider_item(
    state: &mut ExecutionState,
    item: &NewItem,
) -> Result<(), BufferLimitError> {
    let policy = AppendPolicy::cand_1();
    let next_count = state.provider_item_count.saturating_add(1);
    let next_payload_bytes = policy
        .check_item_count(next_count)
        .and_then(|()| policy.accumulate_payload_bytes(state.provider_payload_bytes, item))?;
    if !matches!(item, NewItem::Terminal(_)) {
        policy.reserve_durability_terminal(next_count, next_payload_bytes)?;
    }
    state.provider_item_count = next_count;
    state.provider_payload_bytes = next_payload_bytes;
    Ok(())
}

/// Applies one durable provider item and reports whether it closes the Turn.
pub(super) fn accept_appended_provider_item(
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

/// Advances the in-memory lifecycle through one persisted terminal outcome.
pub(in crate::application) fn apply_terminal_outcome(
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

/// Converts a history failure into the public Turn failure contract.
pub(in crate::application) fn history_failure(
    error: HistoryError,
    accepted: bool,
    published: &[Item],
) -> TurnRunError {
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

/// Marks a post-accept history failure as durable when required.
pub(in crate::application) fn post_accept_failure(
    error: TurnRunError,
    published: &[Item],
) -> TurnRunError {
    match error {
        TurnRunError::History(history_error) => history_failure(history_error, true, published),
        other => other,
    }
}

/// Moves an append outage into recovery-pending before surfacing it.
pub(in crate::application) fn recover_append_failure(
    state: &mut ExecutionState,
    error: HistoryError,
) -> TurnRunError {
    if error == HistoryError::Unavailable
        && let Ok(recovery_pending) = state.lifecycle.recovery_pending()
    {
        state.lifecycle = recovery_pending;
    }
    history_failure(error, true, &state.published)
}
