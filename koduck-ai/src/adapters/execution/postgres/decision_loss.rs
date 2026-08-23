// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Loser-side D-6 terminal and expiry classification under the Turn lock.

use sqlx::Row;

use crate::application::{ApprovalDecisionResolution, ApprovalStoreError};
use crate::domain::TenantId;
use crate::domain::execution::{ApprovalId, ApprovalStatus};

use super::{
    decision_from_code, emit_decision_audit, existing_terminal_resolution,
    interruption_owns_requested_approval, millis, row_version, status_from_code,
};

/// Classifies a lost decision transition without releasing its canonical Turn lock.
pub(super) async fn classify(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    requester_subject: &str,
    thread_id: crate::domain::ThreadId,
    approval_id: ApprovalId,
    decided_at_millis: u64,
) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
    let existing = sqlx::query(
        "SELECT status, decision, version FROM tool_approvals
         WHERE tenant_id = $1 AND approval_id = $2
           AND requester_subject = $3 AND thread_id = $4",
    )
    .bind(tenant_id.as_str())
    .bind(approval_id.as_uuid())
    .bind(requester_subject)
    .bind(thread_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    let Some(row) = existing else {
        return Ok(ApprovalDecisionResolution::NotFound);
    };
    let status_text: String = row
        .try_get("status")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    if status_text != "requested" {
        return Ok(ApprovalDecisionResolution::ExistingTerminal {
            decision: row
                .try_get::<Option<String>, _>("decision")
                .map_err(|_| ApprovalStoreError::Unavailable)?
                .as_deref()
                .and_then(decision_from_code),
            status: status_from_code(&status_text).ok_or(ApprovalStoreError::Unavailable)?,
            version: row_version(&row)?,
        });
    }

    if let Some(expired) = try_expire_requested(
        transaction,
        tenant_id,
        requester_subject,
        thread_id,
        approval_id,
        decided_at_millis,
    )
    .await?
    {
        return Ok(expired);
    }
    if interruption_owns_approval(
        transaction,
        tenant_id,
        requester_subject,
        thread_id,
        approval_id,
    )
    .await?
    {
        return Ok(ApprovalDecisionResolution::TurnGuardRejected);
    }
    let terminal = sqlx::query(
        "SELECT status, decision, version FROM tool_approvals
         WHERE tenant_id = $1 AND approval_id = $2
           AND requester_subject = $3 AND thread_id = $4",
    )
    .bind(tenant_id.as_str())
    .bind(approval_id.as_uuid())
    .bind(requester_subject)
    .bind(thread_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    terminal
        .as_ref()
        .map(existing_terminal_resolution)
        .transpose()?
        .ok_or(ApprovalStoreError::Unavailable)
}

/// Attempts the requested-to-expired transition while retaining the Turn lock.
async fn try_expire_requested(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    requester_subject: &str,
    thread_id: crate::domain::ThreadId,
    approval_id: ApprovalId,
    decided_at_millis: u64,
) -> Result<Option<ApprovalDecisionResolution>, ApprovalStoreError> {
    let expired = sqlx::query(
        "UPDATE tool_approvals
         SET status = 'expired', version = version + 1
         WHERE tenant_id = $1 AND approval_id = $2
           AND requester_subject = $3 AND thread_id = $4
           AND status = 'requested' AND expires_at_millis <= $5
           AND NOT EXISTS (
               SELECT 1 FROM turns owner
               WHERE owner.tenant_id = $1 AND owner.thread_id = $4
                 AND owner.turn_id = tool_approvals.turn_id
                 AND owner.interrupting
           )
         RETURNING version, thread_id, turn_id, attempt_id, \
                    lease_generation, descriptor_id, descriptor_version, \
                    action_digest, profile_id, profile_version",
    )
    .bind(tenant_id.as_str())
    .bind(approval_id.as_uuid())
    .bind(requester_subject)
    .bind(thread_id.as_uuid())
    .bind(millis(decided_at_millis)?)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    let Some(expired_row) = expired else {
        return Ok(None);
    };
    let version = row_version(&expired_row)?;
    emit_decision_audit(
        transaction,
        approval_id,
        ApprovalStatus::Expired,
        None,
        version,
        decided_at_millis,
        tenant_id,
        &expired_row,
    )
    .await?;
    Ok(Some(ApprovalDecisionResolution::ExistingTerminal {
        decision: None,
        status: ApprovalStatus::Expired,
        version,
    }))
}

/// Reports whether interruption owns the still-requested D-6 terminal.
async fn interruption_owns_approval(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    requester_subject: &str,
    thread_id: crate::domain::ThreadId,
    approval_id: ApprovalId,
) -> Result<bool, ApprovalStoreError> {
    let owner = sqlx::query(
        "SELECT approval.status, owner.interrupting
         FROM tool_approvals approval
         JOIN turns owner
           ON owner.tenant_id = approval.tenant_id
          AND owner.thread_id = approval.thread_id
          AND owner.turn_id = approval.turn_id
         WHERE approval.tenant_id = $1 AND approval.approval_id = $2
           AND approval.requester_subject = $3 AND approval.thread_id = $4",
    )
    .bind(tenant_id.as_str())
    .bind(approval_id.as_uuid())
    .bind(requester_subject)
    .bind(thread_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| ApprovalStoreError::Unavailable)?;
    let Some(owner) = owner else {
        return Ok(false);
    };
    let status: String = owner
        .try_get("status")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    let interrupting: bool = owner
        .try_get("interrupting")
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    Ok(interruption_owns_requested_approval(&status, interrupting))
}
