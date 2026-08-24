// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical-row reconciliation for conditional D-7 attempt writes.

use sqlx::{PgPool, Row};

use crate::application::tool_projection::output_digest;
use crate::application::{
    AttemptStoreError, AttemptTerminalResolution, CanonicalAttemptTerminal, EffectState,
    ExecutionFailure, ToolExecutionOutcome, ToolProjection,
};
use crate::domain::execution::{AttemptId, ExactActionBinding, ExecutionStatus};

use super::attempts::{effect_code, hex_digest, millis};

/// Re-reads the canonical D-7 after a terminal write loses or is ambiguous.
pub(super) async fn resolve_terminal_loss(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<AttemptTerminalResolution, AttemptStoreError> {
    let existing = sqlx::query(
        "SELECT thread_id, turn_id, lease_generation, descriptor_id,
                descriptor_version, effect, action_digest, profile_id,
                profile_version, prepared_at_millis, status, version,
                effect_state, failure_code, output
         FROM tool_execution_attempts
         WHERE tenant_id = $1 AND attempt_id = $2",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.attempt_id().as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = existing else {
        return Ok(AttemptTerminalResolution::NotFound);
    };
    if !immutable_fields_match(&row, binding, None) {
        return Ok(AttemptTerminalResolution::Conflict);
    }
    let status = row_status(&row)?;
    if matches!(
        status,
        ExecutionStatus::Succeeded
            | ExecutionStatus::Failed
            | ExecutionStatus::TimedOut
            | ExecutionStatus::Cancelled
    ) {
        let version = row_version(&row)?;
        if version != 3 {
            return Ok(AttemptTerminalResolution::Conflict);
        }
        let canonical = CanonicalAttemptTerminal::from_persistence(
            binding.clone(),
            version,
            canonical_outcome(&row, status)?,
        )
        .map_err(|_| AttemptStoreError::Unavailable)?;
        return Ok(AttemptTerminalResolution::ExistingTerminal(Box::new(
            canonical,
        )));
    }
    if bound_lease_is_not_current(pool, binding).await? {
        return Ok(AttemptTerminalResolution::Fenced);
    }
    Ok(AttemptTerminalResolution::Conflict)
}

/// Reports whether the D-7's bound canonical C-6 lease is not current.
///
/// A missing lease leaves D-7 ownership unproven and therefore fails closed,
/// the same way a mismatched, fenced, or expired lease does.
pub(super) async fn bound_lease_is_not_current(
    pool: &PgPool,
    binding: &ExactActionBinding,
) -> Result<bool, AttemptStoreError> {
    let generation = millis(binding.lease_generation().get())?;
    sqlx::query_scalar(
        "SELECT NOT EXISTS (
             SELECT 1 FROM turn_leases lease
             WHERE lease.tenant_id = $1 AND lease.thread_id = $2 AND lease.turn_id = $3
               AND lease.generation = $4 AND NOT lease.fenced
               AND lease.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP
         )",
    )
    .bind(binding.tenant_id().as_str())
    .bind(binding.thread_id().as_uuid())
    .bind(binding.turn_id().as_uuid())
    .bind(generation)
    .fetch_one(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)
}

/// Verifies the immutable binding fields against one canonical row.
pub(super) fn immutable_fields_match(
    row: &sqlx::postgres::PgRow,
    binding: &ExactActionBinding,
    expected_prepared_at: Option<u64>,
) -> bool {
    let action = binding.action();
    // Decode numeric canonical values fail-closed before comparing, so a
    // drifted negative durable value never compares equal to its expected
    // positive counterpart.
    (|| {
        let lease_generation = canonical_non_negative(row, "lease_generation").ok()?;
        let prepared_at = canonical_non_negative(row, "prepared_at_millis").ok()?;
        Some(
            row.try_get::<uuid::Uuid, _>("thread_id").ok()? == binding.thread_id().as_uuid()
                && row.try_get::<uuid::Uuid, _>("turn_id").ok()? == binding.turn_id().as_uuid()
                && lease_generation == binding.lease_generation().get()
                && expected_prepared_at.is_none_or(|expected| prepared_at == expected)
                && row.try_get::<String, _>("descriptor_id").ok()? == action.descriptor_id()
                && row.try_get::<String, _>("descriptor_version").ok()?
                    == action.descriptor_version()
                && row.try_get::<String, _>("effect").ok()? == effect_code(action.effect())
                && row.try_get::<String, _>("action_digest").ok()?
                    == hex_digest(binding.action_digest().as_bytes())
                && row.try_get::<String, _>("profile_id").ok()? == binding.profile_id()
                && row.try_get::<String, _>("profile_version").ok()? == binding.profile_version(),
        )
    })()
    .unwrap_or(false)
}

/// Rebuilds the canonical bounded outcome from one terminal row.
fn canonical_outcome(
    row: &sqlx::postgres::PgRow,
    status: ExecutionStatus,
) -> Result<ToolExecutionOutcome, AttemptStoreError> {
    let effect_state = row
        .try_get::<Option<String>, _>("effect_state")
        .map_err(|_| AttemptStoreError::Unavailable)?
        .as_deref()
        .and_then(EffectState::from_code)
        .ok_or(AttemptStoreError::Unavailable)?;
    match status {
        ExecutionStatus::Succeeded => Ok(ToolExecutionOutcome::Succeeded {
            output: row
                .try_get::<Option<Vec<u8>>, _>("output")
                .map_err(|_| AttemptStoreError::Unavailable)?
                .ok_or(AttemptStoreError::Unavailable)?,
            effect_state,
        }),
        ExecutionStatus::Failed => Ok(ToolExecutionOutcome::Failed {
            code: row
                .try_get::<Option<String>, _>("failure_code")
                .map_err(|_| AttemptStoreError::Unavailable)?
                .as_deref()
                .and_then(ExecutionFailure::from_stable_code)
                .ok_or(AttemptStoreError::Unavailable)?,
            effect_state,
        }),
        ExecutionStatus::TimedOut => Ok(ToolExecutionOutcome::TimedOut { effect_state }),
        ExecutionStatus::Cancelled => Ok(ToolExecutionOutcome::Cancelled { effect_state }),
        // A prepared or running row is not a terminal and carries no outcome.
        ExecutionStatus::Prepared | ExecutionStatus::Running => Err(AttemptStoreError::Unavailable),
    }
}

/// Rebuilds the D-3 terminal projection from one validated canonical D-7 row.
pub(super) fn terminal_projection(
    row: &sqlx::postgres::PgRow,
) -> Result<ToolProjection, AttemptStoreError> {
    let status = row_status(row)?;
    if !matches!(
        status,
        ExecutionStatus::Succeeded
            | ExecutionStatus::Failed
            | ExecutionStatus::TimedOut
            | ExecutionStatus::Cancelled
    ) || row_version(row)? != 3
    {
        return Err(AttemptStoreError::Unavailable);
    }
    let attempt_id = AttemptId::from_uuid(
        row.try_get("attempt_id")
            .map_err(|_| AttemptStoreError::Unavailable)?,
    );
    let outcome = canonical_outcome(row, status)?;
    let (code, effect_state, output_bytes, output_digest) = match outcome {
        ToolExecutionOutcome::Succeeded {
            output,
            effect_state,
        } => (
            None,
            effect_state,
            u64::try_from(output.len()).map_err(|_| AttemptStoreError::Unavailable)?,
            Some(output_digest(&output)),
        ),
        ToolExecutionOutcome::Failed { code, effect_state } => (Some(code), effect_state, 0, None),
        ToolExecutionOutcome::TimedOut { effect_state }
        | ToolExecutionOutcome::Cancelled { effect_state } => (None, effect_state, 0, None),
    };
    Ok(ToolProjection::ToolResult {
        attempt_id,
        status,
        code,
        effect_state,
        output_bytes,
        output_digest,
        version: 3,
    })
}

fn canonical_non_negative(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> Result<u64, AttemptStoreError> {
    let value = row
        .try_get::<i64, _>(column)
        .map_err(|_| AttemptStoreError::Unavailable)?;
    u64::try_from(value).map_err(|_| AttemptStoreError::Unavailable)
}

/// Reads one canonical D-7 status code.
pub(super) fn row_status(
    row: &sqlx::postgres::PgRow,
) -> Result<ExecutionStatus, AttemptStoreError> {
    let status = row
        .try_get::<String, _>("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    status_from_code(&status).ok_or(AttemptStoreError::Unavailable)
}

/// Reads one canonical D-7 version after validating its positive domain.
pub(super) fn row_version(row: &sqlx::postgres::PgRow) -> Result<u64, AttemptStoreError> {
    let version = row
        .try_get::<i64, _>("version")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    u64::try_from(version)
        .ok()
        .filter(|version| *version >= 1)
        .ok_or(AttemptStoreError::Unavailable)
}

fn status_from_code(code: &str) -> Option<ExecutionStatus> {
    match code {
        "prepared" => Some(ExecutionStatus::Prepared),
        "running" => Some(ExecutionStatus::Running),
        "succeeded" => Some(ExecutionStatus::Succeeded),
        "failed" => Some(ExecutionStatus::Failed),
        "timed_out" => Some(ExecutionStatus::TimedOut),
        "cancelled" => Some(ExecutionStatus::Cancelled),
        _ => None,
    }
}
