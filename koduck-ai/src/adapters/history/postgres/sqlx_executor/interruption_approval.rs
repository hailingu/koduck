// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-6 projection recovery for foreground Turn interruption.

use crate::application::{AppendPolicy, HistoryError, NewItem};
use crate::domain::{TenantId, ThreadId, TurnId};

/// Locks every canonical D-6 terminal and returns each exact projection that
/// is not yet durable beneath the active Turn.
pub(super) async fn unprojected_terminals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<Vec<NewItem>, HistoryError> {
    super::super::approval_terminal_backfill::unprojected_terminals(
        transaction,
        tenant_id,
        thread_id,
        turn_id,
    )
    .await
}

/// Accounts recovered D-6 projections against the remaining CAND-1 Turn
/// item and serialized-payload budgets before any terminal is inserted.
pub(super) fn consume_budget(
    items: &[NewItem],
    mut item_count: usize,
    mut payload_bytes: usize,
) -> Result<(usize, usize), HistoryError> {
    let policy = AppendPolicy::cand_1();
    for item in items {
        item_count = item_count.checked_add(1).ok_or(HistoryError::Unavailable)?;
        policy
            .check_item_count(item_count)
            .map_err(|_| HistoryError::Unavailable)?;
        payload_bytes = policy
            .accumulate_payload_bytes(payload_bytes, item)
            .map_err(|_| HistoryError::Unavailable)?;
    }
    Ok((item_count, payload_bytes))
}
