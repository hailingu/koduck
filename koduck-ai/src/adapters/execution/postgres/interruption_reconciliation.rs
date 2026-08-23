// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical reread for an ambiguous interruption-cancellation commit.

use sqlx::Row;

use crate::application::{ApprovalStoreError, PendingApprovalCancellation};
use crate::domain::execution::{ApprovalId, ExactActionBinding};

use super::{effect_code, hex_digest, millis, row_version};

/// Accepts only the exact interruption-owned cancelled D-6 tuple after a
/// failed `COMMIT` acknowledgement; every other state remains unavailable.
pub(super) async fn reread(
    pool: &sqlx::PgPool,
    binding: &ExactActionBinding,
    approval_id: ApprovalId,
) -> Result<PendingApprovalCancellation, ApprovalStoreError> {
    let action = binding.action();
    let row = sqlx::query(
        "SELECT status, decision, approver, decided_at_millis, version
         FROM tool_approvals
         WHERE tenant_id = $1 AND approval_id = $2 AND thread_id = $3
           AND turn_id = $4 AND attempt_id = $5 AND lease_generation = $6
           AND descriptor_id = $7 AND descriptor_version = $8
           AND effect = $9 AND action_digest = $10
           AND profile_id = $11 AND profile_version = $12",
    )
    .bind(binding.tenant_id().as_str())
    .bind(approval_id.as_uuid())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(binding.attempt_id().as_uuid())
    .bind(millis(binding.lease_generation().get())?)
    .bind(action.descriptor_id())
    .bind(action.descriptor_version())
    .bind(effect_code(action.effect()))
    .bind(hex_digest(binding.action_digest().as_bytes()))
    .bind(binding.profile_id())
    .bind(binding.profile_version())
    .fetch_optional(pool)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    let Some(row) = row else {
        return Err(ApprovalStoreError::Unavailable);
    };
    let interruption_owned = row
        .try_get::<String, _>("status")
        .map_err(|_| ApprovalStoreError::Unavailable)?
        == "cancelled"
        && row
            .try_get::<Option<String>, _>("decision")
            .map_err(|_| ApprovalStoreError::Unavailable)?
            .is_none()
        && row
            .try_get::<Option<String>, _>("approver")
            .map_err(|_| ApprovalStoreError::Unavailable)?
            .is_none()
        && row
            .try_get::<Option<i64>, _>("decided_at_millis")
            .map_err(|_| ApprovalStoreError::Unavailable)?
            .is_none()
        && row_version(&row)? == 2;
    interruption_owned
        .then_some(PendingApprovalCancellation::Cancelled)
        .ok_or(ApprovalStoreError::Unavailable)
}
