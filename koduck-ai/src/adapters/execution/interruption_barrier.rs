// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Loser-side classification for conditional C-5 interruption barriers.

use sqlx::{PgPool, Row};

use crate::application::AttemptStoreError;
use crate::domain::{TenantId, ThreadId, TurnId};

/// One decoded canonical Turn/lease owner-state probe row.
struct OwnerState {
    status: String,
    interrupting: bool,
    fenced: bool,
    within_window: bool,
}

/// Decodes one fail-closed owner-state column.
fn column<'r, T>(row: &'r sqlx::postgres::PgRow, name: &str) -> Result<T, AttemptStoreError>
where
    T: sqlx::types::Type<sqlx::Postgres> + sqlx::decode::Decode<'r, sqlx::Postgres>,
{
    row.try_get(name)
        .map_err(|_| AttemptStoreError::Unavailable)
}

/// Probes the canonical owner state for one exact Turn identity.
///
/// Both barrier classifications read the same joined Turn/lease row, so the
/// probe owns the shared query and fail-closed column decoding.
async fn owner_state(
    pool: &PgPool,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<Option<OwnerState>, AttemptStoreError> {
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
    row.map(|row| {
        Ok(OwnerState {
            status: column(&row, "status")?,
            interrupting: column(&row, "interrupting")?,
            fenced: column(&row, "fenced")?,
            within_window: column(&row, "within_window")?,
        })
    })
    .transpose()
}

/// Reports whether a lost C-5 barrier update is explained by a Turn the
/// history boundary must classify rather than a store outage.
pub(super) async fn lost_to_non_dispatchable_turn(
    pool: &PgPool,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<bool, AttemptStoreError> {
    Ok(
        match owner_state(pool, tenant_id, thread_id, turn_id).await? {
            None => true,
            Some(state) => state.status != "started" || state.fenced || !state.within_window,
        },
    )
}

/// Proves that the exact active C-5 barrier committed before its statement
/// acknowledgement was lost.
pub(super) async fn committed_active_barrier(
    pool: &PgPool,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<bool, AttemptStoreError> {
    Ok(
        match owner_state(pool, tenant_id, thread_id, turn_id).await? {
            None => false,
            Some(state) => active_barrier_matches(
                &state.status,
                state.interrupting,
                state.fenced,
                state.within_window,
            ),
        },
    )
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
