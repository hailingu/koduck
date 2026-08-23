// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Closes and audits the active D-7 attempts of an expired Turn inside the
//! expiry-recovery transaction (ADR-0003 TC-10/TC-14).

use sqlx::{PgConnection, Row};

use crate::application::{
    HistoryError, MAX_ACTION_DURATION_MILLIS, ToolAuditRecord, ToolExecutionOutcome,
};
use crate::domain::execution::{ApprovalStatus, AttemptId, ExecutionStatus};
use crate::domain::{Item, ItemPayload, ToolEffectState};

use super::LeaseKey;
use super::sqlx_executor::{milliseconds_i64, unavailable};

mod terminal_backfill;

use terminal_backfill::backfill_unrecorded_terminal_attempts;

pub(super) async fn close_active_attempts(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
) -> Result<Option<Vec<Item>>, HistoryError> {
    // An expired lease is not evidence that a remote running action stopped.
    // Recovery has no isolated-executor cancellation channel, so preserve a
    // running D-7 until its bounded action deadline has elapsed.
    if running_attempt_deadline_pending(connection, key, terminal_at_millis).await? {
        return Ok(None);
    }
    // An interrupted replica can commit its D-7 terminal and lose the
    // process-local handoff before history receives the D-3 item. Recover
    // those already-terminal rows before closing the remaining live attempts:
    // once the Turn terminal commits, no later recovery may append them.
    let mut projections = backfill_unrecorded_terminal_attempts(connection, key).await?;
    let closed = close_attempt_rows(connection, key, terminal_at_millis).await?;
    projections.reserve(closed.len());
    for attempt in closed {
        projections
            .push(record_closed_attempt(connection, key, terminal_at_millis, attempt).await?);
    }
    Ok(Some(projections))
}

/// Reports whether a running D-7 still has time before its bounded deadline.
async fn running_attempt_deadline_pending(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
) -> Result<bool, HistoryError> {
    let running_starts = sqlx::query_scalar::<_, i64>(
        "SELECT started_at_millis
         FROM tool_execution_attempts
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
           AND status = 'running'
         FOR UPDATE",
    )
    .bind(key.tenant_id.as_str())
    .bind(key.thread_id.as_uuid())
    .bind(key.turn_id.as_uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(unavailable)?;
    let deadline_pending = running_starts.iter().any(|started_at_millis| {
        u64::try_from(*started_at_millis).map_or(true, |started_at_millis| {
            terminal_at_millis < started_at_millis.saturating_add(MAX_ACTION_DURATION_MILLIS)
        })
    });
    Ok(deadline_pending)
}

/// Closes prepared and deadline-expired running D-7 rows in the recovery transaction.
async fn close_attempt_rows(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
) -> Result<Vec<sqlx::postgres::PgRow>, HistoryError> {
    // RETURNING exposes each closed attempt's persisted correlation fields so
    // the same transaction also emits its bounded audit record: the crash
    // path this recovery closes is exactly the path that needs operator
    // evidence, and committing both atomically keeps the every-terminal audit
    // contract true for recovered attempts (ADR-0003 TC-14).
    sqlx::query(
        "UPDATE tool_execution_attempts
         SET status = CASE
                 WHEN status = 'prepared' THEN 'cancelled'
                 ELSE 'timed_out'
             END,
             effect_state = CASE
                 WHEN status = 'prepared' THEN 'not_started'
                 ELSE 'unknown'
             END,
             terminal_at_millis = $4,
             version = 3
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
           AND status IN ('prepared', 'running')
         RETURNING attempt_id, descriptor_id, descriptor_version, profile_id,
                   profile_version, action_digest, lease_generation,
                   effect_state",
    )
    .bind(key.tenant_id.as_str())
    .bind(key.thread_id.as_uuid())
    .bind(key.turn_id.as_uuid())
    .bind(milliseconds_i64(terminal_at_millis)?)
    .fetch_all(&mut *connection)
    .await
    .map_err(unavailable)
}

/// Appends audit evidence and returns the D-3 terminal projection for one closed D-7.
async fn record_closed_attempt(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
    attempt: sqlx::postgres::PgRow,
) -> Result<Item, HistoryError> {
    let attempt_id: uuid::Uuid = attempt.try_get("attempt_id").map_err(unavailable)?;
    let descriptor_id: String = attempt.try_get("descriptor_id").map_err(unavailable)?;
    let descriptor_version: String = attempt.try_get("descriptor_version").map_err(unavailable)?;
    let profile_id: String = attempt.try_get("profile_id").map_err(unavailable)?;
    let profile_version: String = attempt.try_get("profile_version").map_err(unavailable)?;
    let action_digest: String = attempt.try_get("action_digest").map_err(unavailable)?;
    let lease_generation: i64 = attempt.try_get("lease_generation").map_err(unavailable)?;
    let effect_state: String = attempt.try_get("effect_state").map_err(unavailable)?;
    let (outcome, status, projection_effect_state) = if effect_state == "not_started" {
        (
            ToolExecutionOutcome::Cancelled {
                effect_state: crate::application::EffectState::NotStarted,
            },
            ExecutionStatus::Cancelled,
            ToolEffectState::NotStarted,
        )
    } else {
        (
            ToolExecutionOutcome::TimedOut {
                effect_state: crate::application::EffectState::Unknown,
            },
            ExecutionStatus::TimedOut,
            ToolEffectState::Unknown,
        )
    };
    let record = ToolAuditRecord::lease_recovery_terminal(
        &key.tenant_id,
        key.thread_id,
        key.turn_id,
        &crate::domain::execution::AttemptId::from_uuid(attempt_id),
        &descriptor_id,
        &descriptor_version,
        &profile_id,
        &profile_version,
        &action_digest,
        u64::try_from(lease_generation).map_err(|_| HistoryError::Unavailable)?,
        &outcome,
        terminal_at_millis,
    );
    let serialized = crate::adapters::audit::serialize_audit_record(&record)
        .map_err(|_| HistoryError::Unavailable)?;
    sqlx::query(
        "INSERT INTO tool_audit_records \
         (tenant_id, thread_id, turn_id, at_millis, record) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(record.tenant_id())
    .bind(key.thread_id.as_uuid())
    .bind(key.turn_id.as_uuid())
    .bind(milliseconds_i64(terminal_at_millis)?)
    .bind(serialized)
    .execute(&mut *connection)
    .await
    .map_err(unavailable)?;
    Ok(Item::new(
        1,
        ItemPayload::ToolResult {
            attempt_id: Some(AttemptId::from_uuid(attempt_id)),
            status,
            code: None,
            effect_state: Some(projection_effect_state),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        },
    ))
}

/// Cancels requested D-6 records and recovers every missing terminal projection.
///
/// Recovery owns the terminal transition rather than a C-7 approval decision.
/// It records the terminal-owned cancellation without inventing an approver or
/// decision. After that transition it locks every canonical D-6 terminal and
/// returns each exact version that is still absent from D-3, including a
/// decision or expiry that committed before its process lost the projection
/// handoff. The caller appends those views before the bound D-7 and Turn
/// terminals in the same transaction (ADR-0003 TC-06/TC-10/TC-14).
pub(super) async fn recover_approval_terminals(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
) -> Result<Vec<Item>, HistoryError> {
    let cancelled = sqlx::query(
        "UPDATE tool_approvals
         SET status = 'cancelled', version = version + 1
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
           AND status = 'requested'
         RETURNING approval_id, thread_id, turn_id, attempt_id, descriptor_id,
                   descriptor_version, profile_id, profile_version, action_digest,
                   lease_generation, version",
    )
    .bind(key.tenant_id.as_str())
    .bind(key.thread_id.as_uuid())
    .bind(key.turn_id.as_uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(unavailable)?;
    for approval in cancelled {
        let approval_id: uuid::Uuid = approval.try_get("approval_id").map_err(unavailable)?;
        let thread_id: uuid::Uuid = approval.try_get("thread_id").map_err(unavailable)?;
        let turn_id: uuid::Uuid = approval.try_get("turn_id").map_err(unavailable)?;
        let attempt_id: uuid::Uuid = approval.try_get("attempt_id").map_err(unavailable)?;
        let descriptor_id: String = approval.try_get("descriptor_id").map_err(unavailable)?;
        let descriptor_version: String = approval
            .try_get("descriptor_version")
            .map_err(unavailable)?;
        let profile_id: String = approval.try_get("profile_id").map_err(unavailable)?;
        let profile_version: String = approval.try_get("profile_version").map_err(unavailable)?;
        let action_digest: String = approval.try_get("action_digest").map_err(unavailable)?;
        let lease_generation: i64 = approval.try_get("lease_generation").map_err(unavailable)?;
        let version: i64 = approval.try_get("version").map_err(unavailable)?;
        let record = ToolAuditRecord::approval_resolution_from_persisted(
            &key.tenant_id,
            crate::domain::ThreadId::from_uuid(thread_id),
            crate::domain::TurnId::from_uuid(turn_id),
            &crate::domain::execution::AttemptId::from_uuid(attempt_id),
            crate::domain::execution::ApprovalId::from_uuid(approval_id),
            &descriptor_id,
            &descriptor_version,
            &profile_id,
            &profile_version,
            &action_digest,
            u64::try_from(lease_generation).map_err(|_| HistoryError::Unavailable)?,
            ApprovalStatus::Cancelled,
            None,
            u64::try_from(version).map_err(|_| HistoryError::Unavailable)?,
            terminal_at_millis,
        );
        let serialized = crate::adapters::audit::serialize_audit_record(&record)
            .map_err(|_| HistoryError::Unavailable)?;
        sqlx::query(
            "INSERT INTO tool_audit_records
             (tenant_id, thread_id, turn_id, at_millis, record)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(record.tenant_id())
        .bind(thread_id)
        .bind(turn_id)
        .bind(milliseconds_i64(terminal_at_millis)?)
        .bind(serialized)
        .execute(&mut *connection)
        .await
        .map_err(unavailable)?;
    }
    super::approval_terminal_backfill::unprojected_terminals(
        connection,
        &key.tenant_id,
        key.thread_id,
        key.turn_id,
    )
    .await
    .map(|projections| {
        projections
            .into_iter()
            .map(|projection| Item::new(1, projection.into_payload()))
            .collect()
    })
}
