// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Prepared-only conditional close SQL for the canonical D-7 store.
//!
//! The close is a compare-and-set from `prepared`: it may cancel only a row
//! that no other claimant has claimed, so a racing owner that already claimed
//! or terminalized this exact identity keeps its canonical state — and its
//! truthful result — untouched (ADR-0003 TC-10/TC-12).

use sqlx::PgPool;

use crate::application::{AttemptStoreError, PreparedCloseResolution};
use crate::domain::execution::{ExactActionBinding, ExecutionStatus};

use super::attempts::{
    bound_lease_is_not_current, effect_code, hex_digest, immutable_fields_match, millis,
    row_status, row_version,
};

/// Executes one prepared-only conditional close against the canonical store.
///
/// The winner update binds the full immutable record and requires the bound
/// lease to be current, exactly like the terminal commit; the loser side is
/// classified without changing canonical state.
pub(super) async fn close_prepared_row(
    pool: &PgPool,
    binding: &ExactActionBinding,
    cancelled_at_millis: u64,
) -> Result<PreparedCloseResolution, AttemptStoreError> {
    // The close and its correlated audit append commit atomically, exactly
    // like every other D-7 terminal: this transition bypasses the audited
    // commit_terminal path, but its committed cancelled terminal carries the
    // same TC-14 evidence requirement (ADR-0003).
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let action = binding.action();
    let winner = sqlx::query(
        "UPDATE tool_execution_attempts
         SET status = 'cancelled', effect_state = 'not_started',
             terminal_at_millis = $2, version = 3
         WHERE tenant_id = $1 AND attempt_id = $3 AND status = 'prepared'
           AND thread_id = $4 AND turn_id = $5 AND lease_generation = $6
           AND descriptor_id = $7 AND descriptor_version = $8
           AND effect = $9 AND action_digest = $10
           AND profile_id = $11 AND profile_version = $12
           AND EXISTS (
               SELECT 1
               FROM (
                   SELECT generation, fenced, expires_at
                   FROM turn_leases
                   WHERE tenant_id = $1 AND thread_id = $4 AND turn_id = $5
                   FOR UPDATE
               ) AS bound_lease
               WHERE bound_lease.generation = $6 AND NOT bound_lease.fenced
                 AND bound_lease.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
           )
         RETURNING version",
    )
    .bind(binding.tenant_id().as_str())
    .bind(millis(cancelled_at_millis)?)
    .bind(binding.attempt_id().as_uuid())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(millis(binding.lease_generation().get())?)
    .bind(action.descriptor_id())
    .bind(action.descriptor_version())
    .bind(effect_code(action.effect()))
    .bind(hex_digest(binding.action_digest().as_bytes()))
    .bind(binding.profile_id())
    .bind(binding.profile_version())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    if let Some(row) = winner {
        let version = row_version(&row)?;
        if version == 3 {
            let terminal = crate::application::DurableAttemptTerminal::from_outcome(
                &crate::application::ToolExecutionOutcome::Cancelled {
                    effect_state: crate::application::EffectState::NotStarted,
                },
            );
            super::attempts::append_terminal_audit_pub(
                &mut transaction,
                binding,
                &terminal,
                cancelled_at_millis,
            )
            .await?;
            transaction
                .commit()
                .await
                .map_err(|_| AttemptStoreError::Unavailable)?;
            return Ok(PreparedCloseResolution::Won { version });
        }
        return Err(AttemptStoreError::Unavailable);
    }
    drop(transaction);
    resolve_prepared_close_loss(pool, binding).await
}

/// Classifies a lost prepared-only close without changing canonical state.
async fn resolve_prepared_close_loss(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<PreparedCloseResolution, AttemptStoreError> {
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
        return Err(AttemptStoreError::Unavailable);
    };
    if !immutable_fields_match(&row, binding, None) {
        return Err(AttemptStoreError::IdentityConflict);
    }
    let status = row_status(&row)?;
    if status != ExecutionStatus::Prepared {
        // Another owner claimed or terminalized this exact identity: its
        // canonical state must survive untouched.
        return Ok(PreparedCloseResolution::Progressed {
            status,
            version: row_version(&row)?,
        });
    }
    if bound_lease_is_not_current(pool, binding).await? {
        return Ok(PreparedCloseResolution::Fenced);
    }
    Err(AttemptStoreError::Unavailable)
}
