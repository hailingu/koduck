// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Rebuilds missing D-3 terminal projections from canonical D-7 rows.

use sqlx::{PgConnection, Row};

use crate::application::tool_projection::{output_digest, tool_effect_state};
use crate::application::{EffectState, ExecutionFailure, HistoryError, NewItem};
use crate::domain::Item;
use crate::domain::execution::{AttemptId, ExecutionStatus};

use super::super::LeaseKey;
use super::super::sqlx_executor::unavailable;

/// Returns D-7 terminal projections absent from D-3 history for one recovery
/// transaction, in the canonical terminal order.
pub(super) async fn backfill_unrecorded_terminal_attempts(
    connection: &mut PgConnection,
    key: &LeaseKey,
) -> Result<Vec<Item>, HistoryError> {
    unprojected_terminal_attempts(connection, &key.tenant_id, key.thread_id, key.turn_id)
        .await
        .map(|projections| {
            projections
                .into_iter()
                .map(|projection| Item::new(1, projection.into_payload()))
                .collect()
        })
}

/// Locks and rebuilds the complete set of terminal D-7 projections still
/// absent from D-3 for one tenant-owned Turn.
pub(in crate::adapters::history::postgres) async fn unprojected_terminal_attempts(
    connection: &mut PgConnection,
    tenant_id: &crate::domain::TenantId,
    thread_id: crate::domain::ThreadId,
    turn_id: crate::domain::TurnId,
) -> Result<Vec<NewItem>, HistoryError> {
    let rows = sqlx::query(
        "SELECT attempts.attempt_id, attempts.status, attempts.effect_state, \
                attempts.failure_code, attempts.output \
         FROM tool_execution_attempts attempts \
         WHERE attempts.tenant_id = $1 \
           AND attempts.thread_id = $2 AND attempts.turn_id = $3 \
           AND attempts.status IN ('succeeded', 'failed', 'timed_out', 'cancelled') \
           AND NOT EXISTS ( \
               SELECT 1 FROM turn_items items \
               WHERE items.tenant_id = attempts.tenant_id \
                 AND items.thread_id = attempts.thread_id \
                 AND items.turn_id = attempts.turn_id \
                 AND items.item_type = 'tool_result' \
                 AND items.payload::jsonb ->> 'attempt_id' = attempts.attempt_id::text \
                 AND items.payload::jsonb ->> 'version' = attempts.version::text \
           ) \
         ORDER BY attempts.terminal_at_millis, attempts.attempt_id \
         FOR UPDATE OF attempts",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(unavailable)?;
    rows.into_iter()
        .map(|row| {
            let effect_state = row
                .try_get::<Option<String>, _>("effect_state")
                .map_err(unavailable)?;
            let failure_code = row
                .try_get::<Option<String>, _>("failure_code")
                .map_err(unavailable)?;
            let output = row
                .try_get::<Option<Vec<u8>>, _>("output")
                .map_err(unavailable)?;
            terminal_projection(
                AttemptId::from_uuid(row.try_get("attempt_id").map_err(unavailable)?),
                terminal_status(&row.try_get::<String, _>("status").map_err(unavailable)?)?,
                terminal_effect_state(effect_state.as_deref())?,
                failure_code.as_deref(),
                output.as_deref(),
            )
        })
        .collect()
}

/// Rebuilds one D-3 terminal item from a validated canonical D-7 terminal.
fn terminal_projection(
    attempt_id: AttemptId,
    status: ExecutionStatus,
    effect_state: EffectState,
    failure_code: Option<&str>,
    output: Option<&[u8]>,
) -> Result<NewItem, HistoryError> {
    let (code, output_bytes, output_digest) = match status {
        ExecutionStatus::Succeeded => {
            let output = output.ok_or(HistoryError::Unavailable)?;
            if failure_code.is_some() {
                return Err(HistoryError::Unavailable);
            }
            (
                None,
                u64::try_from(output.len()).map_err(|_| HistoryError::Unavailable)?,
                Some(output_digest(output)),
            )
        }
        ExecutionStatus::Failed => (
            Some(
                failure_code
                    .and_then(ExecutionFailure::from_stable_code)
                    .ok_or(HistoryError::Unavailable)?
                    .stable_code()
                    .to_owned(),
            ),
            0,
            None,
        ),
        ExecutionStatus::TimedOut | ExecutionStatus::Cancelled => {
            if failure_code.is_some() || output.is_some() {
                return Err(HistoryError::Unavailable);
            }
            (None, 0, None)
        }
        ExecutionStatus::Prepared | ExecutionStatus::Running => {
            return Err(HistoryError::Unavailable);
        }
    };
    Ok(NewItem::ToolResult {
        attempt_id: Some(attempt_id),
        status,
        code,
        effect_state: Some(tool_effect_state(effect_state)),
        output_bytes,
        output_digest,
        version: Some(3),
    })
}

/// Parses one persisted terminal status without accepting active D-7 states.
fn terminal_status(status: &str) -> Result<ExecutionStatus, HistoryError> {
    match status {
        "succeeded" => Ok(ExecutionStatus::Succeeded),
        "failed" => Ok(ExecutionStatus::Failed),
        "timed_out" => Ok(ExecutionStatus::TimedOut),
        "cancelled" => Ok(ExecutionStatus::Cancelled),
        _ => Err(HistoryError::Unavailable),
    }
}

/// Parses the required executor effect evidence of one terminal D-7 row.
fn terminal_effect_state(value: Option<&str>) -> Result<EffectState, HistoryError> {
    value
        .and_then(EffectState::from_code)
        .ok_or(HistoryError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::terminal_projection;
    use crate::application::{EffectState, NewItem};
    use crate::domain::ToolEffectState;
    use crate::domain::execution::{AttemptId, ExecutionStatus};

    #[test]
    fn a_previously_closed_cancellation_backfills_its_terminal_projection() {
        let attempt_id = AttemptId::new();

        let projection = terminal_projection(
            attempt_id,
            ExecutionStatus::Cancelled,
            EffectState::NotStarted,
            None,
            None,
        )
        .expect("a canonical cancellation has a D-3 terminal projection");

        assert!(matches!(
            projection,
            NewItem::ToolResult {
                attempt_id: Some(projected_id),
                status: ExecutionStatus::Cancelled,
                effect_state: Some(ToolEffectState::NotStarted),
                version: Some(3),
                ..
            } if projected_id == attempt_id
        ));
    }
}
