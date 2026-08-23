// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Requested D-6 insertion serialized with canonical Turn ownership.

use sqlx::{PgPool, Postgres, Transaction};

use crate::application::{ApprovalInsertResolution, ApprovalStoreError};
use crate::domain::execution::ApprovalRequest;

use super::{SqlxApprovalRecordStore, effect_code, hex_digest, millis};

/// Inserts one requested D-6 only while its exact Turn and lease remain
/// dispatchable, while preserving idempotent canonical replay.
pub(super) async fn insert(
    pool: &PgPool,
    request: &ApprovalRequest,
    requester_subject: &str,
) -> Result<ApprovalInsertResolution, ApprovalStoreError> {
    if let Some(existing) = replay_from_pool(pool, request, requester_subject).await? {
        return Ok(existing);
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| ApprovalStoreError::Unavailable)?;
    if !lock_current_owner(&mut transaction, request).await? {
        drop(transaction);
        return replay_from_pool(pool, request, requester_subject)
            .await?
            .ok_or(ApprovalStoreError::Unavailable);
    }
    if insert_locked(&mut transaction, request, requester_subject).await? {
        if transaction.commit().await.is_err() {
            return replay_from_pool(pool, request, requester_subject)
                .await?
                .ok_or(ApprovalStoreError::Unavailable);
        }
        return Ok(ApprovalInsertResolution::Inserted);
    }
    replay_from_transaction(&mut transaction, request, requester_subject)
        .await?
        .ok_or(ApprovalStoreError::Unavailable)
}

/// Locks and validates the same Turn/lease owner used by D-7 preparation and interruption.
async fn lock_current_owner(
    transaction: &mut Transaction<'_, Postgres>,
    request: &ApprovalRequest,
) -> Result<bool, ApprovalStoreError> {
    let binding = request.binding();
    sqlx::query(
        "SELECT 1 FROM turns owner JOIN turn_leases lease
           ON lease.tenant_id = owner.tenant_id
          AND lease.thread_id = owner.thread_id
          AND lease.turn_id = owner.turn_id
         WHERE owner.tenant_id = $1 AND owner.thread_id = $2 AND owner.turn_id = $3
           AND owner.status = 'started' AND NOT owner.interrupting
           AND lease.generation = $4 AND NOT lease.fenced
           AND lease.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
         FOR UPDATE OF owner, lease",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(millis(binding.lease_generation().get())?)
    .fetch_optional(&mut **transaction)
    .await
    .map(|owner| owner.is_some())
    .map_err(|_| ApprovalStoreError::Unavailable)
}

/// Inserts the immutable requested row while its owner locks remain held.
async fn insert_locked(
    transaction: &mut Transaction<'_, Postgres>,
    request: &ApprovalRequest,
    requester_subject: &str,
) -> Result<bool, ApprovalStoreError> {
    let binding = request.binding();
    let action = binding.action();
    sqlx::query(
        "INSERT INTO tool_approvals (
            tenant_id, approval_id, requester_subject, thread_id, turn_id,
            attempt_id, lease_generation, descriptor_id, descriptor_version,
            effect, action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis, status, version
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
            'requested', 1
        ) ON CONFLICT (tenant_id, approval_id) DO NOTHING",
    )
    .bind(binding.tenant_id().as_str())
    .bind(request.approval_id().as_uuid())
    .bind(requester_subject)
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
    .bind(millis(request.requested_at_millis())?)
    .bind(millis(request.expires_at_millis())?)
    .execute(&mut **transaction)
    .await
    .map(|outcome| outcome.rows_affected() == 1)
    .map_err(|_| ApprovalStoreError::Unavailable)
}

async fn replay_from_pool(
    pool: &PgPool,
    request: &ApprovalRequest,
    requester_subject: &str,
) -> Result<Option<ApprovalInsertResolution>, ApprovalStoreError> {
    replay(
        sqlx::query(existing_query())
            .bind(request.tenant_id().as_str())
            .bind(request.approval_id().as_uuid())
            .fetch_optional(pool),
        request,
        requester_subject,
    )
    .await
}

async fn replay_from_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    request: &ApprovalRequest,
    requester_subject: &str,
) -> Result<Option<ApprovalInsertResolution>, ApprovalStoreError> {
    replay(
        sqlx::query(existing_query())
            .bind(request.tenant_id().as_str())
            .bind(request.approval_id().as_uuid())
            .fetch_optional(&mut **transaction),
        request,
        requester_subject,
    )
    .await
}

async fn replay(
    query: impl std::future::Future<Output = Result<Option<sqlx::postgres::PgRow>, sqlx::Error>>,
    request: &ApprovalRequest,
    requester_subject: &str,
) -> Result<Option<ApprovalInsertResolution>, ApprovalStoreError> {
    query
        .await
        .map_err(|_| ApprovalStoreError::Unavailable)?
        .as_ref()
        .map(|row| SqlxApprovalRecordStore::conflict_resolution(row, request, requester_subject))
        .transpose()
}

const fn existing_query() -> &'static str {
    "SELECT requester_subject, thread_id, turn_id, attempt_id,
            lease_generation, descriptor_id, descriptor_version, effect,
            action_digest, profile_id, profile_version,
            requested_at_millis, expires_at_millis, status, decision, version
     FROM tool_approvals
     WHERE tenant_id = $1 AND approval_id = $2"
}
