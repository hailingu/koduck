// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Provider-item accounting and durable-failure conversion for one Turn.

use crate::domain::{Item, TerminalOutcome};

use super::super::{
    AppendPolicy, BufferLimitError, DurabilityFailure, HistoryError, NewItem, ResourceLimitFailure,
    TurnHistory, TurnRunError,
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
pub(in crate::application) fn accept_appended_provider_item(
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

/// Accounts one provider item and reports whether its rejection closed the
/// Turn through the resource-limit terminal (ADR-0005 PLB-5/PLB-7).
pub(in crate::application) fn enforce_provider_limit<H: TurnHistory>(
    history: &mut H,
    accepted: &super::super::ports::AcceptedTurn,
    state: &mut ExecutionState,
    item: &NewItem,
) -> Result<bool, TurnRunError> {
    match validate_provider_item(state, item) {
        Ok(()) => Ok(false),
        Err(_) => terminalize_from_limit(history, accepted, state),
    }
}

/// Durably closes a budget-exhausted Turn as `RESOURCE_LIMIT_EXCEEDED` and
/// surfaces the typed resource-limit failure.
///
/// A persisted interruption that wins the terminal arbitration still closes
/// the Turn as interrupted; an append outage keeps the durability path.
pub(in crate::application) fn terminalize_from_limit<H: TurnHistory>(
    history: &mut H,
    accepted: &super::super::ports::AcceptedTurn,
    state: &mut ExecutionState,
) -> Result<bool, TurnRunError> {
    close_as_durable_failure(
        history,
        accepted,
        state,
        super::super::durability::RESOURCE_LIMIT_TERMINAL_CODE,
        true,
    )
}

/// Durably closes a Turn through the bounded durability-failure terminal for
/// a projection or executor failure that is not count or payload exhaustion.
///
/// The appended terminal stays unpublished, exactly as before the
/// resource-limit split: the durability path surfaces through recovery and
/// the `durability-unavailable` delivery mapping (ADR-0001, ADR-0005 PLB-7).
pub(in crate::application) fn terminalize_from_projection_failure<H: TurnHistory>(
    history: &mut H,
    accepted: &super::super::ports::AcceptedTurn,
    state: &mut ExecutionState,
) -> Result<bool, TurnRunError> {
    let terminal = history
        .append_provider_terminal(
            accepted,
            TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            },
        )
        .map_err(|error| recover_append_failure(state, error))?;
    match &terminal.payload {
        crate::domain::ItemPayload::Terminal(TerminalOutcome::Interrupted) => {
            apply_terminal_outcome(state, &TerminalOutcome::Interrupted)?;
            state.published.push(terminal);
            Ok(true)
        }
        crate::domain::ItemPayload::Terminal(TerminalOutcome::Failed { code })
            if code == "DURABILITY_UNAVAILABLE" =>
        {
            Err(history_failure(
                HistoryError::Unavailable,
                true,
                &state.published,
            ))
        }
        _ => Err(HistoryError::Unavailable.into()),
    }
}

/// Appends one bounded failure terminal and converts it into the owning
/// failure surface: `resource_limit` selects the typed resource-limit
/// failure, otherwise the failure maps to durability unavailability.
fn close_as_durable_failure<H: TurnHistory>(
    history: &mut H,
    accepted: &super::super::ports::AcceptedTurn,
    state: &mut ExecutionState,
    code: &'static str,
    resource_limit: bool,
) -> Result<bool, TurnRunError> {
    let terminal = history
        .append_provider_terminal(
            accepted,
            TerminalOutcome::Failed {
                code: code.to_owned(),
            },
        )
        .map_err(|error| recover_append_failure(state, error))?;
    match &terminal.payload {
        crate::domain::ItemPayload::Terminal(TerminalOutcome::Interrupted) => {
            apply_terminal_outcome(state, &TerminalOutcome::Interrupted)?;
            state.published.push(terminal);
            Ok(true)
        }
        crate::domain::ItemPayload::Terminal(TerminalOutcome::Failed { code: actual })
            if actual == code =>
        {
            apply_terminal_outcome(
                state,
                &TerminalOutcome::Failed {
                    code: actual.clone(),
                },
            )?;
            state.published.push(terminal);
            if resource_limit {
                Err(TurnRunError::ResourceLimit(ResourceLimitFailure {
                    published: state.published.clone(),
                }))
            } else {
                Err(history_failure(
                    HistoryError::Unavailable,
                    true,
                    &state.published,
                ))
            }
        }
        _ => Err(HistoryError::Unavailable.into()),
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
