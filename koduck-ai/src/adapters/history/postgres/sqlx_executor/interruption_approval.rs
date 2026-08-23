// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-6 projection recovery for foreground Turn interruption.

use sqlx::Row;

use crate::application::{AppendPolicy, HistoryError, NewItem};
use crate::domain::execution::{ApprovalId, ApprovalStatus, AttemptId};
use crate::domain::{TenantId, ThreadId, TurnId};

use super::unavailable;

/// Locks canonical interruption-owned D-6 cancellations and returns each
/// projection that is not yet durable beneath the active Turn.
pub(super) async fn unprojected_cancellations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<Vec<NewItem>, HistoryError> {
    let rows = sqlx::query(
        "SELECT approval.approval_id, approval.attempt_id, approval.version,
                EXISTS (
                    SELECT 1 FROM turn_items item
                    WHERE item.tenant_id = approval.tenant_id
                      AND item.thread_id = approval.thread_id
                      AND item.turn_id = approval.turn_id
                      AND item.item_type = 'approval_status'
                      AND item.payload::JSONB ->> 'approval_id' = approval.approval_id::TEXT
                      AND item.payload::JSONB ->> 'status' = 'cancelled'
                      AND item.payload::JSONB ->> 'version' = approval.version::TEXT
                ) AS projected
         FROM tool_approvals approval
         WHERE approval.tenant_id = $1 AND approval.thread_id = $2
           AND approval.turn_id = $3 AND approval.status = 'cancelled'
           AND approval.decision IS NULL
         ORDER BY approval.approval_id
         FOR UPDATE OF approval",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(unavailable)?;
    let mut projections = Vec::with_capacity(rows.len());
    for row in rows {
        let version = u64::try_from(row.try_get::<i64, _>("version").map_err(unavailable)?)
            .map_err(|_| HistoryError::Unavailable)?;
        if version
            != crate::application::tool_projection::approval_version(ApprovalStatus::Cancelled)
        {
            return Err(HistoryError::Unavailable);
        }
        if !row.try_get::<bool, _>("projected").map_err(unavailable)? {
            projections.push(NewItem::ApprovalStatus {
                approval_id: ApprovalId::from_uuid(
                    row.try_get("approval_id").map_err(unavailable)?,
                ),
                attempt_id: AttemptId::from_uuid(row.try_get("attempt_id").map_err(unavailable)?),
                status: ApprovalStatus::Cancelled,
                decision: None,
                version,
            });
        }
    }
    Ok(projections)
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
