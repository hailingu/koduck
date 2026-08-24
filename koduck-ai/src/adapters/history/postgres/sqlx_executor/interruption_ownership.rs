// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authenticated Thread resolution for paired C-5 interruptions.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::adapters::history::postgres::attempt_recovery::terminal_backfill;
use crate::application::{HistoryError, NewItem};
use crate::domain::{TenantId, ThreadId, TrustContext, TurnId};

use super::{interruption_approval, interruption_commit, is_terminal_status, unavailable};

/// Conditionally records D-7 terminals followed by the authenticated Turn
/// interruption terminal for one active tenant-owned Turn.
pub(super) async fn request(
    pool: &PgPool,
    trust: &TrustContext,
    turn_id: TurnId,
    tool_terminals: Vec<NewItem>,
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
    let first_sequence = u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?;
    let (item_count, payload_bytes) = interruption_budget(
        &mut transaction,
        trust,
        ThreadId::from_uuid(thread_id),
        turn_id,
    )
    .await?;
    let approval_cancellations = interruption_approval::unprojected_terminals(
        &mut transaction,
        &trust.tenant_id,
        ThreadId::from_uuid(thread_id),
        turn_id,
    )
    .await?;
    let (item_count, payload_bytes) =
        interruption_approval::consume_budget(&approval_cancellations, item_count, payload_bytes)?;
    validate_interruption_terminals(&tool_terminals, item_count, payload_bytes)?;
    validate_complete_canonical_interruption_terminals(
        &mut transaction,
        &trust.tenant_id,
        ThreadId::from_uuid(thread_id),
        turn_id,
        &tool_terminals,
    )
    .await?;
    let projection_count = interruption_commit::append_items(
        &mut transaction,
        &trust.tenant_id,
        ThreadId::from_uuid(thread_id),
        turn_id,
        first_sequence,
        &approval_cancellations,
        &tool_terminals,
    )
    .await?;
    sqlx::query(
        "UPDATE turns SET interrupt_requested = TRUE, status = 'interrupted', \
         next_sequence = next_sequence + $5 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
         AND next_sequence = $4 AND status = 'started'",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id)
    .bind(turn_id.as_uuid())
    .bind(sequence)
    .bind(i64::try_from(projection_count + 1).map_err(|_| HistoryError::Unavailable)?)
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

/// Measures the CAND-1 provider-item budget already consumed by the active
/// Turn while its row lock prevents a concurrent append from changing it.
async fn interruption_budget(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    trust: &TrustContext,
    thread_id: ThreadId,
    turn_id: TurnId,
) -> Result<(usize, usize), HistoryError> {
    let row = sqlx::query(
        "SELECT COUNT(*) FILTER (WHERE item_type <> 'user_message') AS item_count, \
                COALESCE(SUM(octet_length(payload)) FILTER (WHERE item_type <> 'user_message'), 0) \
                    AS payload_bytes \
         FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(unavailable)?;
    let item_count = row.try_get::<i64, _>("item_count").map_err(unavailable)?;
    let payload_bytes = row
        .try_get::<i64, _>("payload_bytes")
        .map_err(unavailable)?;
    Ok((
        usize::try_from(item_count).map_err(|_| HistoryError::Unavailable)?,
        usize::try_from(payload_bytes).map_err(|_| HistoryError::Unavailable)?,
    ))
}

/// Fails closed unless public C-5 interruption items match the canonical D-7
/// projection tuple and fit the active Turn's remaining CAND-1 budget.
fn validate_interruption_terminals(
    items: &[NewItem],
    item_count: usize,
    payload_bytes: usize,
) -> Result<(), HistoryError> {
    crate::application::validate_interruption_terminals(items, item_count, payload_bytes)
        .map_err(|_| HistoryError::Unavailable)
}

/// Locks the complete missing D-7 terminal set and rejects any public batch
/// that is empty, incomplete, stale, extra, or tuple-drifted.
async fn validate_complete_canonical_interruption_terminals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    items: &[NewItem],
) -> Result<(), HistoryError> {
    let canonical = terminal_backfill::unprojected_terminal_attempts(
        transaction,
        tenant_id,
        thread_id,
        turn_id,
    )
    .await?;
    if canonical.len() != items.len()
        || canonical
            .iter()
            .any(|expected| !items.iter().any(|supplied| supplied == expected))
    {
        return Err(HistoryError::Unavailable);
    }
    Ok(())
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
    use super::{committed_interruption_is_recoverable, validate_interruption_terminals};
    use crate::application::{HistoryError, NewItem};
    use crate::domain::ToolEffectState;
    use crate::domain::execution::{AttemptId, ExecutionStatus};

    #[test]
    fn a_durable_interrupted_turn_reconciles_a_lost_commit_acknowledgement() {
        assert!(committed_interruption_is_recoverable("interrupted"));
        assert!(!committed_interruption_is_recoverable("started"));
        assert!(!committed_interruption_is_recoverable("failed"));
    }

    #[test]
    fn interruption_rejects_a_running_tool_result() {
        let item = NewItem::ToolResult {
            attempt_id: Some(AttemptId::new()),
            status: ExecutionStatus::Running,
            code: None,
            effect_state: Some(ToolEffectState::Started),
            output_bytes: 0,
            output_digest: None,
            version: Some(2),
        };

        assert_eq!(
            validate_interruption_terminals(&[item], 0, 0),
            Err(HistoryError::Unavailable),
        );
    }

    #[test]
    fn interruption_rejects_duplicate_terminal_identities() {
        let terminal = cancelled_terminal();

        assert_eq!(
            validate_interruption_terminals(&[terminal.clone(), terminal], 0, 0),
            Err(HistoryError::Unavailable),
        );
    }

    #[test]
    fn interruption_rejects_terminals_past_the_turn_item_budget() {
        assert_eq!(
            validate_interruption_terminals(&[cancelled_terminal()], 64, 0),
            Err(HistoryError::Unavailable),
        );
    }

    #[test]
    fn interruption_reserves_one_item_for_its_turn_terminal() {
        assert_eq!(
            validate_interruption_terminals(&[], 64, 0),
            Err(HistoryError::Unavailable),
        );
    }

    #[test]
    fn interruption_reserves_payload_bytes_for_its_turn_terminal() {
        assert_eq!(
            validate_interruption_terminals(&[], 0, 1_048_575),
            Err(HistoryError::Unavailable),
        );
    }

    fn cancelled_terminal() -> NewItem {
        NewItem::ToolResult {
            attempt_id: Some(AttemptId::new()),
            status: ExecutionStatus::Cancelled,
            code: None,
            effect_state: Some(ToolEffectState::NotStarted),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        }
    }
}
