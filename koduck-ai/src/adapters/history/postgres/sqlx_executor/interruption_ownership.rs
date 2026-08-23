// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authenticated Thread resolution for paired C-5 interruptions.

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::application::{HistoryError, NewItem};
use crate::domain::{
    Item, ItemPayload, TenantId, TerminalOutcome, ThreadId, ToolEffectState, TrustContext, TurnId,
};

use super::{insert_item, is_terminal_status, unavailable};

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
    validate_interruption_terminals(&tool_terminals, item_count, payload_bytes)?;
    validate_canonical_interruption_terminals(
        &mut transaction,
        &trust.tenant_id,
        ThreadId::from_uuid(thread_id),
        turn_id,
        &tool_terminals,
    )
    .await?;
    for (offset, terminal) in tool_terminals.iter().enumerate() {
        let item = Item::new(
            first_sequence
                .checked_add(offset as u64)
                .ok_or(HistoryError::Unavailable)?,
            terminal.clone().into_payload(),
        );
        insert_item(
            &mut transaction,
            &trust.tenant_id,
            ThreadId::from_uuid(thread_id),
            turn_id,
            &item,
        )
        .await?;
    }
    let terminal_sequence = first_sequence
        .checked_add(tool_terminals.len() as u64)
        .ok_or(HistoryError::Unavailable)?;
    let item = Item::new(
        terminal_sequence,
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
         next_sequence = next_sequence + $5 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
         AND next_sequence = $4 AND status = 'started'",
    )
    .bind(trust.tenant_id.as_str())
    .bind(thread_id)
    .bind(turn_id.as_uuid())
    .bind(sequence)
    .bind(i64::try_from(tool_terminals.len() + 1).map_err(|_| HistoryError::Unavailable)?)
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

/// Locks and verifies every supplied interruption terminal against the exact
/// canonical D-7 row owned by this tenant, Thread, and Turn.
async fn validate_canonical_interruption_terminals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    items: &[NewItem],
) -> Result<(), HistoryError> {
    for item in items {
        let NewItem::ToolResult {
            attempt_id: Some(attempt_id),
            ..
        } = item
        else {
            return Err(HistoryError::Unavailable);
        };
        let row = sqlx::query(
            "SELECT status, effect_state, failure_code, output, version
             FROM tool_execution_attempts
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3
               AND attempt_id = $4
             FOR UPDATE",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(attempt_id.as_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?;
        let Some(row) = row else {
            return Err(HistoryError::Unavailable);
        };
        if !canonical_terminal_matches(item, &row)? {
            return Err(HistoryError::Unavailable);
        }
    }
    Ok(())
}

/// Compares one public D-3 terminal tuple with its locked canonical D-7 row.
fn canonical_terminal_matches(
    item: &NewItem,
    row: &sqlx::postgres::PgRow,
) -> Result<bool, HistoryError> {
    let NewItem::ToolResult {
        status,
        code,
        effect_state: Some(effect_state),
        output_bytes,
        output_digest,
        version: Some(version),
        ..
    } = item
    else {
        return Ok(false);
    };
    let output = row
        .try_get::<Option<Vec<u8>>, _>("output")
        .map_err(unavailable)?;
    let canonical_output_bytes = output
        .as_ref()
        .map_or(Ok(0), |bytes| u64::try_from(bytes.len()))
        .map_err(|_| HistoryError::Unavailable)?;
    let canonical_output_digest = output.as_deref().map(crate::application::output_digest);
    let canonical_version = u64::try_from(row.try_get::<i64, _>("version").map_err(unavailable)?)
        .map_err(|_| HistoryError::Unavailable)?;
    Ok(
        row.try_get::<String, _>("status").map_err(unavailable)? == status.as_str()
            && row
                .try_get::<Option<String>, _>("effect_state")
                .map_err(unavailable)?
                .as_deref()
                == Some(effect_state_code(*effect_state))
            && row
                .try_get::<Option<String>, _>("failure_code")
                .map_err(unavailable)?
                == *code
            && canonical_output_bytes == *output_bytes
            && canonical_output_digest == *output_digest
            && canonical_version == *version,
    )
}

/// Returns the stable D-7 representation of one public Tool effect state.
const fn effect_state_code(effect_state: ToolEffectState) -> &'static str {
    match effect_state {
        ToolEffectState::NotStarted => "not_started",
        ToolEffectState::Started => "started",
        ToolEffectState::Unknown => "unknown",
    }
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
