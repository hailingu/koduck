// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical D-6 terminal projections recovered before a Turn terminal.

use sqlx::Row;

use crate::application::{HistoryError, NewItem};
use crate::domain::execution::{ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId};
use crate::domain::{TenantId, ThreadId, TurnId};

use super::payload_codec::{parse_approval_decision, parse_approval_status};

/// Locks and returns every canonical D-6 terminal whose exact version has not
/// yet been appended beneath the owning active Turn.
pub(super) async fn unprojected_terminals(
    connection: &mut sqlx::PgConnection,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<Vec<NewItem>, HistoryError> {
    let rows = sqlx::query(
        "SELECT approval.approval_id, approval.attempt_id, approval.status,
                approval.decision, approval.version,
                EXISTS (
                    SELECT 1 FROM turn_items item
                    WHERE item.tenant_id = approval.tenant_id
                      AND item.thread_id = approval.thread_id
                      AND item.turn_id = approval.turn_id
                      AND item.item_type = 'approval_status'
                      AND item.payload::JSONB ->> 'approval_id' = approval.approval_id::TEXT
                      AND item.payload::JSONB ->> 'status' = approval.status
                      AND (item.payload::JSONB ->> 'decision')
                            IS NOT DISTINCT FROM approval.decision
                      AND item.payload::JSONB ->> 'version' = approval.version::TEXT
                ) AS projected
         FROM tool_approvals approval
         WHERE approval.tenant_id = $1 AND approval.thread_id = $2
           AND approval.turn_id = $3
           AND approval.status IN ('accepted', 'declined', 'cancelled', 'expired')
         ORDER BY approval.approval_id
         FOR UPDATE OF approval",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(unavailable)?;
    let mut projections = Vec::with_capacity(rows.len());
    for row in rows {
        let status_text: String = row.try_get("status").map_err(unavailable)?;
        let status = parse_approval_status(&status_text)?;
        let decision = row
            .try_get::<Option<String>, _>("decision")
            .map_err(unavailable)?
            .as_deref()
            .map(parse_approval_decision)
            .transpose()?;
        let version = u64::try_from(row.try_get::<i64, _>("version").map_err(unavailable)?)
            .map_err(|_| HistoryError::Unavailable)?;
        if !canonical_terminal(status, decision, version) {
            return Err(HistoryError::Unavailable);
        }
        if !row.try_get::<bool, _>("projected").map_err(unavailable)? {
            projections.push(NewItem::ApprovalStatus {
                approval_id: ApprovalId::from_uuid(
                    row.try_get("approval_id").map_err(unavailable)?,
                ),
                attempt_id: AttemptId::from_uuid(row.try_get("attempt_id").map_err(unavailable)?),
                status,
                decision,
                version,
            });
        }
    }
    Ok(projections)
}

/// Validates one durable D-6 row before exposing it as a recovered projection.
fn canonical_terminal(
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
) -> bool {
    version == crate::application::tool_projection::approval_version(status)
        && match status {
            ApprovalStatus::Accepted => decision == Some(ApprovalDecision::Accepted),
            ApprovalStatus::Declined => decision == Some(ApprovalDecision::Declined),
            ApprovalStatus::Cancelled => {
                decision.is_none() || decision == Some(ApprovalDecision::Cancelled)
            }
            ApprovalStatus::Expired => decision.is_none(),
            ApprovalStatus::Requested => false,
        }
}

fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}
