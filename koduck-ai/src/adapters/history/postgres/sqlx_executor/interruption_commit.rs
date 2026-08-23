// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Ordered History items committed by one authenticated Turn interruption.

use crate::application::{HistoryError, NewItem};
use crate::domain::{Item, ItemPayload, TenantId, TerminalOutcome, ThreadId, TurnId};

use super::insert_item;

/// Appends recovered D-6 projections, supplied D-7 terminals, and exactly one
/// Turn interruption terminal in that canonical order.
pub(super) async fn append_items(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    first_sequence: u64,
    approval_cancellations: &[NewItem],
    tool_terminals: &[NewItem],
) -> Result<usize, HistoryError> {
    let projection_count = approval_cancellations
        .len()
        .checked_add(tool_terminals.len())
        .ok_or(HistoryError::Unavailable)?;
    for (offset, projection) in approval_cancellations
        .iter()
        .chain(tool_terminals)
        .enumerate()
    {
        let item = Item::new(
            first_sequence
                .checked_add(offset as u64)
                .ok_or(HistoryError::Unavailable)?,
            projection.clone().into_payload(),
        );
        insert_item(transaction, tenant_id, thread_id, turn_id, &item).await?;
    }
    let terminal_sequence = first_sequence
        .checked_add(projection_count as u64)
        .ok_or(HistoryError::Unavailable)?;
    let terminal = Item::new(
        terminal_sequence,
        ItemPayload::Terminal(TerminalOutcome::Interrupted),
    );
    insert_item(transaction, tenant_id, thread_id, turn_id, &terminal).await?;
    Ok(projection_count)
}
