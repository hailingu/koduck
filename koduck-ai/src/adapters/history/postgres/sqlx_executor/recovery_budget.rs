// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Atomic CAND-1 item and payload preflight for Turn terminal recovery.

use sqlx::Row;

use crate::application::{AppendPolicy, HistoryError};
use crate::domain::{Item, ItemPayload, TenantId, TerminalOutcome, ThreadId, TurnId};

use super::{encode_payload, unavailable};

/// Validates all recovered D-3 projections plus the mandatory Turn terminal.
pub(super) async fn validate(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    projections: &[Item],
    terminal: &TerminalOutcome,
) -> Result<(), HistoryError> {
    let row = sqlx::query(
        "SELECT COUNT(*) FILTER (WHERE item_type <> 'user_message') AS item_count, \
                COALESCE(SUM(octet_length(payload)) FILTER \
                    (WHERE item_type <> 'user_message'), 0) AS payload_bytes \
         FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(unavailable)?;
    let item_count = usize::try_from(row.try_get::<i64, _>("item_count").map_err(unavailable)?)
        .map_err(|_| HistoryError::Unavailable)?;
    let payload_bytes = usize::try_from(
        row.try_get::<i64, _>("payload_bytes")
            .map_err(unavailable)?,
    )
    .map_err(|_| HistoryError::Unavailable)?;
    validate_totals(item_count, payload_bytes, projections, terminal)
}

/// Applies the exact CAND-1 count and encoded-payload limits to one recovery batch.
fn validate_totals(
    item_count: usize,
    mut payload_bytes: usize,
    projections: &[Item],
    terminal: &TerminalOutcome,
) -> Result<(), HistoryError> {
    let recovered_count = projections
        .len()
        .checked_add(1)
        .ok_or(HistoryError::Unavailable)?;
    AppendPolicy::cand_1()
        .check_item_count(
            item_count
                .checked_add(recovered_count)
                .ok_or(HistoryError::Unavailable)?,
        )
        .map_err(|_| HistoryError::Unavailable)?;
    for payload in projections
        .iter()
        .map(|item| &item.payload)
        .chain(std::iter::once(&ItemPayload::Terminal(terminal.clone())))
    {
        payload_bytes = payload_bytes
            .checked_add(encode_payload(payload).1.len())
            .ok_or(HistoryError::Unavailable)?;
        AppendPolicy::cand_1()
            .check_payload_bytes(payload_bytes)
            .map_err(|_| HistoryError::Unavailable)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::application::HistoryError;
    use crate::domain::TerminalOutcome;

    use super::validate_totals;

    #[test]
    fn mandatory_terminal_is_included_in_recovery_item_and_payload_budgets() {
        assert_eq!(
            validate_totals(63, 1_048_574, &[], &TerminalOutcome::Cancelled),
            Ok(()),
            "the exact 64-item and 1-MiB boundary remains legal"
        );
        assert_eq!(
            validate_totals(64, 0, &[], &TerminalOutcome::Cancelled),
            Err(HistoryError::Unavailable),
            "the mandatory terminal cannot become item 65"
        );
        assert_eq!(
            validate_totals(0, 1_048_575, &[], &TerminalOutcome::Cancelled),
            Err(HistoryError::Unavailable),
            "the mandatory two-byte terminal payload cannot exceed 1 MiB"
        );
    }
}
