// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Loser-side classification for conditional C-5 interruption barriers.

use sqlx::{PgPool, Row};

use crate::application::AttemptStoreError;
use crate::domain::{TenantId, ThreadId, TurnId};

/// Reports whether a lost C-5 barrier update is explained by a Turn the
/// history boundary must classify rather than a store outage.
pub(super) async fn lost_to_non_dispatchable_turn(
    pool: &PgPool,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<bool, AttemptStoreError> {
    let row = sqlx::query(
        "SELECT t.status, l.fenced, \
         l.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP AS within_window \
         FROM turns t JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
         WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = row else {
        return Ok(true);
    };
    let status: String = row
        .try_get("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let fenced: bool = row
        .try_get("fenced")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let within_window: bool = row
        .try_get("within_window")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    Ok(status != "started" || fenced || !within_window)
}

/// Proves that the exact active C-5 barrier committed before its statement
/// acknowledgement was lost.
pub(super) async fn committed_active_barrier(
    pool: &PgPool,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<bool, AttemptStoreError> {
    let row = sqlx::query(
        "SELECT t.status, t.interrupting, l.fenced, \
         l.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP AS within_window \
         FROM turns t JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
         WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(|_| AttemptStoreError::Unavailable)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let status: String = row
        .try_get("status")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let interrupting: bool = row
        .try_get("interrupting")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let fenced: bool = row
        .try_get("fenced")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    let within_window: bool = row
        .try_get("within_window")
        .map_err(|_| AttemptStoreError::Unavailable)?;
    Ok(active_barrier_matches(
        &status,
        interrupting,
        fenced,
        within_window,
    ))
}

/// Validates the canonical state that identifies an acknowledged C-5 barrier.
fn active_barrier_matches(
    status: &str,
    interrupting: bool,
    fenced: bool,
    within_window: bool,
) -> bool {
    status == "started" && interrupting && !fenced && within_window
}

#[cfg(test)]
mod tests {
    use super::active_barrier_matches;

    #[test]
    fn only_the_exact_active_interruption_barrier_reconciles_an_ambiguous_write() {
        assert!(active_barrier_matches("started", true, false, true));
        assert!(!active_barrier_matches("started", false, false, true));
        assert!(!active_barrier_matches("interrupted", true, false, true));
        assert!(!active_barrier_matches("started", true, true, true));
        assert!(!active_barrier_matches("started", true, false, false));
    }
}
