// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authenticated Thread resolution for paired C-5 interruptions.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::application::HistoryError;
use crate::domain::{Item, ItemPayload, TerminalOutcome, ThreadId, TrustContext, TurnId};

use super::{insert_item, is_terminal_status, unavailable};

/// Conditionally records the authenticated interrupt terminal for one active
/// tenant-owned Turn.
pub(super) async fn request(
    pool: &PgPool,
    trust: &TrustContext,
    turn_id: TurnId,
) -> Result<(), HistoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let ownership = sqlx::query(
        "SELECT t.thread_id, t.status, t.next_sequence, l.fenced, \
         l.expires_at + INTERVAL '2 seconds' > CURRENT_TIMESTAMP AS within_window \
         FROM turns t JOIN threads h USING (tenant_id, thread_id) \
         JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
         WHERE t.tenant_id = $1 AND h.subject_id = $2 AND t.turn_id = $3 FOR UPDATE",
    )
    .bind(trust.tenant_id.as_str())
    .bind(trust.subject_id.as_str())
    .bind(turn_id.as_uuid())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let Some(ownership) = ownership else {
        return Err(HistoryError::NotFound);
    };
    let status: String = ownership.try_get("status").map_err(unavailable)?;
    if is_terminal_status(&status) {
        return Err(HistoryError::AlreadyTerminal);
    }
    let fenced: bool = ownership.try_get("fenced").map_err(unavailable)?;
    let within_window: bool = ownership.try_get("within_window").map_err(unavailable)?;
    if status != "started" || fenced || !within_window {
        return Err(HistoryError::Fenced);
    }
    let thread_id: Uuid = ownership.try_get("thread_id").map_err(unavailable)?;
    let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
    let item = Item::new(
        u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
        ItemPayload::Terminal(TerminalOutcome::Interrupted),
    );
    insert_item(
        &mut transaction,
        &trust.tenant_id,
        ThreadId::from_uuid(thread_id),
        turn_id,
        &item,
    )
    .await?;
    sqlx::query(
        "UPDATE turns SET interrupt_requested = TRUE, status = 'interrupted', \
         next_sequence = next_sequence + 1 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
         AND next_sequence = $4 AND status = 'started'",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id)
    .bind(turn_id.as_uuid())
    .bind(sequence)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if transaction.commit().await.is_err() {
        if recover_committed_interruption(pool, trust, turn_id).await? {
            return Ok(());
        }
        return Err(HistoryError::Unavailable);
    }
    Ok(())
}

/// Proves that an interrupted Turn committed after an ambiguous `COMMIT`
/// acknowledgement for the authenticated owner.
async fn recover_committed_interruption(
    pool: &PgPool,
    trust: &TrustContext,
    turn_id: TurnId,
) -> Result<bool, HistoryError> {
    let status = sqlx::query_scalar::<_, String>(
        "SELECT t.status FROM turns t JOIN threads h USING (tenant_id, thread_id) \
         WHERE t.tenant_id = $1 AND h.subject_id = $2 AND t.turn_id = $3",
    )
    .bind(trust.tenant_id.as_str())
    .bind(trust.subject_id.as_str())
    .bind(turn_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    Ok(status
        .as_deref()
        .is_some_and(committed_interruption_is_recoverable))
}

fn committed_interruption_is_recoverable(status: &str) -> bool {
    status == "interrupted"
}

/// Resolves the authenticated Turn's owning Thread before the runner enters
/// the paired C-5 interruption path.
pub(super) async fn resolve(
    pool: &PgPool,
    trust: &TrustContext,
    turn_id: TurnId,
) -> Result<Option<ThreadId>, HistoryError> {
    let ownership = sqlx::query(
        "SELECT t.thread_id, t.status
         FROM turns t JOIN threads h USING (tenant_id, thread_id)
         WHERE t.tenant_id = $1 AND h.subject_id = $2 AND t.turn_id = $3",
    )
    .bind(trust.tenant_id.as_str())
    .bind(trust.subject_id.as_str())
    .bind(turn_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    let Some(ownership) = ownership else {
        return Err(HistoryError::NotFound);
    };
    let status: String = ownership.try_get("status").map_err(unavailable)?;
    if is_terminal_status(&status) {
        return Err(HistoryError::AlreadyTerminal);
    }
    let thread_id: Uuid = ownership.try_get("thread_id").map_err(unavailable)?;
    Ok(Some(ThreadId::from_uuid(thread_id)))
}

#[cfg(test)]
mod tests {
    use super::committed_interruption_is_recoverable;

    #[test]
    fn a_durable_interrupted_turn_reconciles_a_lost_commit_acknowledgement() {
        assert!(committed_interruption_is_recoverable("interrupted"));
        assert!(!committed_interruption_is_recoverable("started"));
        assert!(!committed_interruption_is_recoverable("failed"));
    }
}
