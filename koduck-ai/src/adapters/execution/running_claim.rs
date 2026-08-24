// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Conditional `prepared -> running` D-7 claim and loser reconciliation.

use sqlx::{PgPool, Row};

use crate::application::{AttemptStoreError, DispatchClaimResolution};
use crate::domain::execution::{ExactActionBinding, ExecutionStatus};

use super::attempt_reconciliation::{
    bound_lease_is_not_current, immutable_fields_match, row_status, row_version,
};
use super::attempts::{effect_code, hex_digest, millis};

/// Claims the sole canonical running slot, reconciling any ambiguous result.
pub(super) async fn claim_running(
    pool: &PgPool,
    binding: &ExactActionBinding,
    started_at_millis: u64,
) -> Result<DispatchClaimResolution, AttemptStoreError> {
    match claim_running_winner(pool, binding, started_at_millis).await {
        Ok(true) => Ok(DispatchClaimResolution::Claimed { version: 2 }),
        // A client-side statement error can arrive after `PostgreSQL`
        // committed the autocommit transition. Re-read the exact canonical
        // row before declaring the claim unavailable, so a recovered
        // running/version-2 row restores its D-3 view without permitting a
        // second executor dispatch.
        Ok(false) | Err(AttemptStoreError::Unavailable) => resolve_claim_loss(pool, binding).await,
        Err(error) => Err(error),
    }
}

/// Runs the fully bound conditional `prepared -> running` update.
async fn claim_running_winner(
    pool: &PgPool,
    binding: &ExactActionBinding,
    started_at_millis: u64,
) -> Result<bool, AttemptStoreError> {
    let action = binding.action();
    claim_running_result(
        sqlx::query(
            "WITH locked_owner AS (
             SELECT t.status, t.interrupting, l.generation, l.fenced, l.expires_at
             FROM turns t JOIN turn_leases l
               ON l.tenant_id = t.tenant_id
              AND l.thread_id = t.thread_id
              AND l.turn_id = t.turn_id
             WHERE t.tenant_id = $1 AND t.thread_id = $4 AND t.turn_id = $5
             FOR UPDATE OF t, l
         )
         UPDATE tool_execution_attempts
         SET status = 'running', started_at_millis = $3, version = 2
         WHERE tenant_id = $1 AND attempt_id = $2 AND status = 'prepared'
           AND thread_id = $4 AND turn_id = $5 AND lease_generation = $6
           AND descriptor_id = $7 AND descriptor_version = $8
           AND effect = $9 AND action_digest = $10
           AND profile_id = $11 AND profile_version = $12
           AND EXISTS (
               SELECT 1 FROM locked_owner
               WHERE status = 'started' AND NOT interrupting
                 AND generation = $6 AND NOT fenced
                 AND expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
           )
           AND NOT EXISTS (
               SELECT 1 FROM tool_execution_attempts other
               WHERE other.tenant_id = $1 AND other.turn_id = $5
                 AND other.status = 'running' AND other.attempt_id <> $2
           )
         RETURNING version",
        )
        .bind(binding.tenant_id().as_str())
        .bind(binding.attempt_id().as_uuid())
        .bind(millis(started_at_millis)?)
        .bind(binding.thread_id().as_uuid())
        .bind(binding.turn_id().as_uuid())
        .bind(millis(binding.lease_generation().get())?)
        .bind(action.descriptor_id())
        .bind(action.descriptor_version())
        .bind(effect_code(action.effect()))
        .bind(hex_digest(binding.action_digest().as_bytes()))
        .bind(binding.profile_id())
        .bind(binding.profile_version())
        .fetch_optional(pool)
        .await,
    )
}

/// Maps a conditional-update response to a winner or a reconciliation read.
fn claim_running_result(
    result: Result<Option<sqlx::postgres::PgRow>, sqlx::Error>,
) -> Result<bool, AttemptStoreError> {
    match result {
        Ok(winner) => Ok(winner.is_some()),
        Err(error)
            if error
                .as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation) =>
        {
            Ok(false)
        }
        Err(_) => Err(AttemptStoreError::Unavailable),
    }
}

/// Re-reads the exact canonical attempt after the conditional claim loses.
async fn resolve_claim_loss(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<DispatchClaimResolution, AttemptStoreError> {
    let existing = sqlx::query(
        "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                descriptor_version, effect, action_digest, profile_id,
                profile_version, prepared_at_millis, status, version
         FROM tool_execution_attempts
         WHERE tenant_id = $1 AND attempt_id = $2",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = existing else {
        return Ok(DispatchClaimResolution::NotFound);
    };
    if !immutable_fields_match(&row, binding, None) {
        return Err(AttemptStoreError::IdentityConflict);
    }
    let status = row_status(&row)?;
    let version = row_version(&row)?;
    match status {
        ExecutionStatus::Running
        | ExecutionStatus::Succeeded
        | ExecutionStatus::Failed
        | ExecutionStatus::TimedOut
        | ExecutionStatus::Cancelled => Ok(DispatchClaimResolution::Existing { status, version }),
        ExecutionStatus::Prepared => match turn_claim_state(pool, binding).await? {
            TurnClaimState::Interrupted => Ok(DispatchClaimResolution::Interrupted),
            TurnClaimState::Inactive => Ok(DispatchClaimResolution::Fenced),
            TurnClaimState::Active if bound_lease_is_not_current(pool, binding).await? => {
                Ok(DispatchClaimResolution::Fenced)
            }
            TurnClaimState::Active if another_attempt_is_running(pool, binding).await? => {
                Ok(DispatchClaimResolution::Concurrent)
            }
            TurnClaimState::Active => Err(AttemptStoreError::Unavailable),
        },
    }
}

/// Durable Turn state relevant to a lost conditional dispatch claim.
enum TurnClaimState {
    /// The Turn remains active and has no durable interruption barrier.
    Active,
    /// An authenticated interruption sealed the active Turn.
    Interrupted,
    /// The Turn is absent or no longer dispatchable.
    Inactive,
}

/// Reads the Turn barrier after a prepared claim loses.
async fn turn_claim_state(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<TurnClaimState, AttemptStoreError> {
    let row = sqlx::query(
        "SELECT status, interrupting FROM turns \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = row else {
        return Ok(TurnClaimState::Inactive);
    };
    let status = row
        .try_get::<String, _>("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let interrupting = row
        .try_get::<bool, _>("interrupting")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    if status != "started" {
        Ok(TurnClaimState::Inactive)
    } else if interrupting {
        Ok(TurnClaimState::Interrupted)
    } else {
        Ok(TurnClaimState::Active)
    }
}

/// Reports whether another canonical D-7 owns this Turn's running slot.
async fn another_attempt_is_running(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<bool, AttemptStoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
               AND status = 'running' AND attempt_id <> $4 \
         )",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(binding.attempt_id().as_uuid())
    .fetch_one(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)
}
