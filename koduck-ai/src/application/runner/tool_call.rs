// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md

//! Servicing of one assembled model Tool call through the C-5 port.
//!
//! The seam owns the durable projection sink lifecycle, the cumulative
//! Turn-budget synchronization, and the terminal ownership rules that decide
//! between continuation, executor failure, and durability boundaries.

use crate::domain::{TerminalOutcome, TrustContext};

use super::ExecutionState;
use super::failure::{
    history_failure, recover_append_failure, terminalize_from_limit,
    terminalize_from_projection_failure,
};
use crate::application::MAX_EXECUTOR_OUTPUT_BYTES;
use crate::application::ToolCallError;
use crate::application::ports::{
    AcceptedTurn, CommittedToolCall, HistoryError, ModelToolCall, ToolCallExecutor,
    ToolCallTurnContext, TurnHistory, TurnRunError, TurnStreamEvent,
};
use crate::application::runner_terminals::append_provider_terminal;
use crate::application::tool_projection::TurnProjectionSink;

/// Services one assembled model Tool call through the C-5 port and records
/// its D-3 items with durable-before-publish ordering.
///
/// A turn-level port failure owns the turn terminal with its stable code; a
/// typed denial or unavailability arrives as recorded items instead of
/// failing the turn (ADR-0003 TC-06/TC-11).
#[allow(
    clippy::too_many_arguments,
    reason = "each parameter is one independently validated orchestration input"
)]
pub(super) fn handle_tool_call<H: TurnHistory, T: ToolCallExecutor>(
    history: &mut H,
    tools: &mut T,
    accepted: &AcceptedTurn,
    trust: &TrustContext,
    state: &mut ExecutionState,
    name: String,
    arguments: String,
    observer: &mut dyn FnMut(TurnStreamEvent),
) -> Result<bool, TurnRunError> {
    let context = ToolCallTurnContext {
        tenant_id: accepted.tenant_id.clone(),
        thread_id: accepted.thread_id,
        turn_id: accepted.turn_id,
        lease_generation: accepted.generation,
    };
    let call = ModelToolCall { name, arguments };
    // The runner supplies the durable projection sink, seeded with the
    // cumulative per-Turn budget counters so every call shares the one
    // 64-item/1-MiB allowance with the provider items: every approval,
    // dispatch, denial, and terminal view is preflighted as a complete
    // sequence, durably appended as it happens — the running view before the
    // executor dispatch — and published to the live observer at its publish
    // boundary (ADR-0001 exact buffer contract, ADR-0003 TC-06).
    let mut projections = TurnProjectionSink::new(
        &mut *history,
        accepted,
        &mut *observer,
        state.provider_item_count,
        state.provider_payload_bytes,
    );
    let result = tools.execute_tool_call(call.clone(), &context, trust, &mut projections);
    // Publish anything an implementation durably appended but did not publish,
    // so no durable projection stays invisible past this call boundary.
    projections.drain_unpublished();
    let projections_failed = projections.is_failed();
    let projections_budget_exhausted = projections.budget_exhausted();
    let terminal_recovery_required = projections.terminal_recovery_required();
    let lifecycle_complete = projections.is_lifecycle_complete();
    let matches_committed_result = result
        .as_ref()
        .is_ok_and(|result| projections.matches_committed_result(result));
    let (provider_item_count, provider_payload_bytes) = projections.budget();
    state.provider_item_count = provider_item_count;
    state.provider_payload_bytes = provider_payload_bytes;
    // Record the durably appended projections in append order. They were
    // already observed at their publish boundaries, so the observation
    // watermark advances past them. The shape is validated by construction
    // (only approval, dispatch, denial, and terminal views exist); a
    // defensive guard still refuses anything else (ADR-0003 TC-06).
    let durable_items = projections.into_durable_items();
    for durable in &durable_items {
        if !matches!(
            durable.payload,
            crate::domain::ItemPayload::ApprovalStatus { .. }
                | crate::domain::ItemPayload::ToolCall { .. }
                | crate::domain::ItemPayload::ToolResult { .. }
        ) {
            return Err(history_failure(
                HistoryError::Unavailable,
                true,
                &state.published,
            ));
        }
    }
    state.published.extend(durable_items);
    state.observed_len = state.published.len();
    if matches!(&result, Err(ToolCallError::Reconciliation(_))) {
        // A C-5 reconciliation requirement proves a canonical D-7 may still
        // be live. It outranks a failed D-3 append: terminalizing the Turn
        // would remove it from recovery scans and strand the live effect.
        return Err(history_failure(
            HistoryError::Unavailable,
            true,
            &state.published,
        ));
    }
    if projections_failed {
        // A projection that was rejected or could not be appended durably —
        // a noncanonical tuple, an out-of-contract sequence exceeding the
        // cumulative per-Turn budget, or an append outage — is a durability
        // boundary violation that outranks any executor error: the Turn
        // terminalizes through the owned limit/recovery path rather than
        // recording a normal tool-error terminal over incomplete history
        // (ADR-0001 exact buffer contract, ADR-0003 TC-06). Count or payload
        // exhaustion closes through the distinct resource-limit terminal
        // while every other projection failure keeps durability-unavailable
        // (ADR-0005 PLB-7).
        if terminal_recovery_required {
            // Production commits D-7 before emitting its terminal D-3 view.
            // A failed terminal projection therefore keeps the Turn
            // recovery-pending: closing it here would exclude it from the
            // only scan that can backfill that canonical terminal.
            return Err(recover_append_failure(state, HistoryError::Unavailable));
        }
        if projections_budget_exhausted {
            return terminalize_from_limit(history, accepted, state);
        }
        return terminalize_from_projection_failure(history, accepted, state);
    }
    let result = match result {
        Ok(result) => result,
        Err(ToolCallError::Reconciliation(_)) => {
            // The C-5 boundary intentionally retains a live D-7 when its
            // canonical terminal cannot be decided. Committing a failed Turn
            // terminal here would strand that D-7: expiry recovery only
            // reconciles non-terminal Turns and interruption rejects a
            // terminal Turn. Surface the durability failure and keep the
            // Turn open so reconciliation closes both (ADR-0003
            // TC-10/TC-12).
            return Err(history_failure(
                HistoryError::Unavailable,
                true,
                &state.published,
            ));
        }
        Err(error) => {
            append_provider_terminal(
                history,
                accepted,
                state,
                TerminalOutcome::Failed {
                    code: error.stable_code().to_owned(),
                },
            )?;
            return Ok(true);
        }
    };
    if !lifecycle_complete || !matches_committed_result {
        return terminalize_from_projection_failure(history, accepted, state);
    }
    // The committed result crosses the public executor port untrusted:
    // enforce the same raw output-byte bound the executor boundary already
    // applies, so an approved at-limit result is never re-measured against an
    // expanded JSON serialization and rejected here, while an
    // out-of-contract implementation still fails closed (ADR-0003
    // TC-09/TC-11).
    if result.content.len() > MAX_EXECUTOR_OUTPUT_BYTES {
        return terminalize_from_projection_failure(history, accepted, state);
    }
    state.current_calls.push(CommittedToolCall { call, result });
    Ok(false)
}
