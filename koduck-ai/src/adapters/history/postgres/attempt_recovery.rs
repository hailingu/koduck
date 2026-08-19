// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Closes and audits the active D-7 attempts of an expired Turn inside the
//! expiry-recovery transaction (ADR-0003 TC-10/TC-14).

use sqlx::{PgConnection, Row};

use crate::application::{HistoryError, ToolAuditRecord, ToolExecutionOutcome};

use super::LeaseKey;
use super::sqlx_executor::{milliseconds_i64, unavailable};

pub(super) async fn close_active_attempts(
    connection: &mut PgConnection,
    key: &LeaseKey,
    terminal_at_millis: u64,
) -> Result<(), HistoryError> {
    // RETURNING exposes each closed attempt's persisted correlation fields so
    // the same transaction also emits its bounded audit record: the crash
    // path this recovery closes is exactly the path that needs operator
    // evidence, and committing both atomically keeps the every-terminal audit
    // contract true for recovered attempts (ADR-0003 TC-14).
    let closed = sqlx::query(
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
    .map_err(unavailable)?;
    for attempt in closed {
        let attempt_id: uuid::Uuid = attempt.try_get("attempt_id").map_err(unavailable)?;
        let descriptor_id: String = attempt.try_get("descriptor_id").map_err(unavailable)?;
        let descriptor_version: String =
            attempt.try_get("descriptor_version").map_err(unavailable)?;
        let profile_id: String = attempt.try_get("profile_id").map_err(unavailable)?;
        let profile_version: String = attempt.try_get("profile_version").map_err(unavailable)?;
        let action_digest: String = attempt.try_get("action_digest").map_err(unavailable)?;
        let lease_generation: i64 = attempt.try_get("lease_generation").map_err(unavailable)?;
        let effect_state: String = attempt.try_get("effect_state").map_err(unavailable)?;
        let outcome = if effect_state == "not_started" {
            ToolExecutionOutcome::Cancelled {
                effect_state: crate::application::EffectState::NotStarted,
            }
        } else {
            ToolExecutionOutcome::TimedOut {
                effect_state: crate::application::EffectState::Unknown,
            }
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
    }
    Ok(())
}
