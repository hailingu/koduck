// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Provider-stream driving for the runner.
//!
//! This seam owns the per-event driving loop: latency-deadline delta flushes,
//! the bounded persisted-interruption poll, consumer cancellation, and the
//! `ProviderEvent` handlers that append durable items or terminals. The
//! lifecycle orchestration that accepts and settles a whole Turn stays in
//! `runner.rs`; both halves share `ExecutionState`.

use std::time::{Duration, Instant};

use crate::domain::{TerminalOutcome, Usage};

use super::ExecutionState;
use super::failure::{
    accept_appended_provider_item, enforce_provider_limit, recover_append_failure,
};
use super::tool_call;
use crate::application::ports::{
    AcceptedTurn, HistoryError, NewItem, ProviderEvent, ToolCallExecutor, TurnHistory,
    TurnRunError, TurnStreamEvent,
};
use crate::application::runner_terminals::{
    append_coalesced_deltas, append_provider_terminal, append_provider_terminal_observed,
    append_terminal, event_terminal_or_recover, flush_buffered_deltas, observe_item,
    publish_replayed_terminal,
};

/// Maximum frequency of persisted interruption checks during provider streams.
const INTERRUPTION_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Appends one provider terminal, adopting the terminal a competing writer
/// already committed when this owner's append is fenced or already terminal.
///
/// Fencing means a durable terminal exists, so replaying it is the only
/// truthful publication left (ADR-0001 durable-before-visible ordering).
pub(super) fn append_terminal_or_replay_fenced<H: TurnHistory>(
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

/// Drives one provider stream to its end, arbitrating every event's durable
/// publication, and returns whether the Turn closed.
///
/// Each event is bounded by the latency-deadline delta flush and the persisted
/// interruption poll before its handler runs; a stream that ends without a
/// terminal either hands its Tool-call round to a continuation (the caller
/// decides) or is closed by the caller's stream-ended terminal.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one independently validated orchestration input"
)]
pub(super) fn drive_stream<H: TurnHistory, T: ToolCallExecutor>(
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
        if drive_stream_event(
            history,
            tools,
            accepted,
            trust,
            state,
            event,
            observer,
            cancelled,
            &mut last_interruption_poll,
        )? {
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

/// Handles one streamed event with the latency-deadline flushes, bounded
/// persisted-interruption poll, consumer cancellation, and observation window
/// the driving loop requires. Returns whether the Turn closed.
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one independently validated orchestration input"
)]
fn drive_stream_event<H: TurnHistory, T: ToolCallExecutor>(
    history: &mut H,
    tools: &mut T,
    accepted: &AcceptedTurn,
    trust: &crate::domain::TrustContext,
    state: &mut ExecutionState,
    event: ProviderEvent,
    observer: &mut dyn FnMut(TurnStreamEvent),
    cancelled: &dyn Fn() -> bool,
    last_interruption_poll: &mut Option<Instant>,
) -> Result<bool, TurnRunError> {
    // The latency deadline is sampled and flushed before any potentially
    // blocking control read and before every event — including a backlog
    // of consecutive Delta frames that never reaches the Pending arm — so
    // a due buffered chunk publishes no later than 500 ms after its first
    // byte, with its outcome arbitrated like any event failure
    // (ADR-0005 PLB-2/PLB-7).
    if flush_due_deltas(history, accepted, state, observer)? {
        return Ok(true);
    }
    let interruption_poll_due = last_interruption_poll
        .is_none_or(|last: Instant| last.elapsed() >= INTERRUPTION_POLL_INTERVAL);
    if interruption_poll_due {
        *last_interruption_poll = Some(Instant::now());
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
    // A control read can block past the boundary a just-preceding sample
    // missed, so the deadline is re-sampled after the blocking read and
    // before the event is handled (ADR-0005 PLB-2).
    if flush_due_deltas(history, accepted, state, observer)? {
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
    Ok(false)
}

/// Samples the latency deadline and flushes any due buffered chunk with its
/// outcome arbitrated (ADR-0005 PLB-2/PLB-7). Returns whether the Turn
/// closed.
fn flush_due_deltas<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    let Some(due) = state.delta_coalescer.take_due_flush(Instant::now()) else {
        return Ok(false);
    };
    let outcome = append_coalesced_deltas(history, accepted, state, [due], observer);
    arbitrate_flush_outcome(history, accepted, state, outcome, observer)
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
            handle_usage_event(history, accepted, state, counters, observer)
        }
        ProviderEvent::Completed => handle_completed_event(history, accepted, state, observer),
        ProviderEvent::Error { code } => {
            handle_error_event(history, accepted, state, code, observer)
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

/// Every request of one Turn — including each Tool-call continuation —
/// reports its own counters; the Turn terminal carries their checked sum
/// (ADR-0003 TC-11). Buffered text flushes durably before the usage boundary
/// (ADR-0005 PLB-3).
fn handle_usage_event<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    counters: Usage,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
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

/// Handles the provider's completion boundary: buffered text flushes durably
/// first (ADR-0005 PLB-3), a stream still owing Tool-call continuation fails
/// closed as a protocol violation (ADR-0003 TC-11), and otherwise the
/// completed Turn terminal is appended under the provider budget.
fn handle_completed_event<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
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

/// Handles a provider error: buffered text flushes durably before the error
/// terminal (ADR-0005 PLB-3), and the failed terminal is appended under the
/// provider budget.
fn handle_error_event<H: TurnHistory>(
    history: &mut H,
    accepted: &AcceptedTurn,
    state: &mut ExecutionState,
    code: String,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    if flush_buffered_deltas(history, accepted, state, observer)? {
        return Ok(true);
    }
    let item = NewItem::Terminal(TerminalOutcome::Failed { code: code.clone() });
    if enforce_provider_limit(history, accepted, state, &item)? {
        return Ok(true);
    }
    append_provider_terminal(history, accepted, state, TerminalOutcome::Failed { code })?;
    Ok(true)
}
