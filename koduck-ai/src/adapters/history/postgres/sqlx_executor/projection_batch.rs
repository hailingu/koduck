// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Atomic `PostgreSQL` persistence for one D-3 Tool-projection sequence.

use sqlx::{PgPool, Row};

use crate::application::{AcceptedTurn, HistoryError};
use crate::domain::Item;

use super::{encode_payload, generation_i64, insert_item, is_terminal_status, unavailable};

/// Commits every non-terminal D-3 item in one transaction, or commits none.
pub(super) async fn append(
    pool: &PgPool,
    turn: &AcceptedTurn,
    mut items: Vec<Item>,
) -> Result<Vec<Item>, HistoryError> {
    if items.is_empty() {
        return Err(HistoryError::Unavailable);
    }
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    super::super::commit_reconciliation::lock_operation(
        &mut transaction,
        items[0].item_id.as_uuid(),
    )
    .await?;
    let ownership = sqlx::query(
        "SELECT t.status, t.next_sequence, t.interrupt_requested, t.interrupting, l.fenced FROM turns t \
         JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
         WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3 \
         AND l.generation = $4 FOR UPDATE",
    )
    .bind(turn.tenant_id.as_str())
    .bind(turn.thread_id.as_uuid())
    .bind(turn.turn_id.as_uuid())
    .bind(generation_i64(turn.generation)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(HistoryError::Fenced)?;
    let status: String = ownership.try_get("status").map_err(unavailable)?;
    let fenced: bool = ownership.try_get("fenced").map_err(unavailable)?;
    let interrupted: bool = ownership
        .try_get("interrupt_requested")
        .map_err(unavailable)?;
    let interrupting: bool = ownership.try_get("interrupting").map_err(unavailable)?;
    if is_terminal_status(&status) {
        return Err(HistoryError::AlreadyTerminal);
    }
    if fenced || status != "started" || interrupted || interrupting {
        return Err(HistoryError::Fenced);
    }
    let first_sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
    for (offset, item) in items.iter_mut().enumerate() {
        let sequence = first_sequence
            .checked_add(i64::try_from(offset).map_err(|_| HistoryError::Unavailable)?)
            .ok_or(HistoryError::Unavailable)?;
        item.sequence = u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?;
        if encode_payload(&item.payload).2 {
            return Err(HistoryError::Unavailable);
        }
        insert_item(
            &mut transaction,
            &turn.tenant_id,
            turn.thread_id,
            turn.turn_id,
            item,
        )
        .await?;
    }
    let next_sequence = first_sequence
        .checked_add(i64::try_from(items.len()).map_err(|_| HistoryError::Unavailable)?)
        .ok_or(HistoryError::Unavailable)?;
    let updated = sqlx::query(
        "UPDATE turns SET next_sequence = $5 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
         AND next_sequence = $4 AND status = 'started'",
    )
    .bind(turn.tenant_id.as_str())
    .bind(turn.thread_id.as_uuid())
    .bind(turn.turn_id.as_uuid())
    .bind(first_sequence)
    .bind(next_sequence)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if updated.rows_affected() != 1 {
        return Err(HistoryError::Fenced);
    }
    transaction.commit().await.map_err(unavailable)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    use crate::application::TurnCommand;
    use crate::domain::execution::{AttemptId, ExecutionStatus};
    use crate::domain::{ItemPayload, TenantId, ThreadId, ToolEffectState, TrustContext, TurnId};

    use super::super::SqlxPostgresExecutor;
    use super::{Item, append};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn committed_projection_reconciles_after_a_lost_acknowledgement() {
        let Ok(database_url) = std::env::var("KODUCK_AI_TEST_DATABASE_URL") else {
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect to disposable PostgreSQL");
        // The shared process-wide guard keeps this harness's DDL from racing
        // the parallel env-gated harnesses in the same test binary.
        crate::test_migrations::ensure(&pool).await;
        let tenant =
            TenantId::new(format!("projection-{}", Uuid::new_v4())).expect("unique tenant");
        let trust = TrustContext::new(tenant, "projection-owner").expect("valid trust context");
        let command = TurnCommand::new(trust, None, "projection lost acknowledgement")
            .expect("valid turn command");
        let executor = SqlxPostgresExecutor::new(pool.clone(), tokio::runtime::Handle::current());
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        let accepted = executor
            .accept_initial_with_identity_async(&command, ThreadId::new(), TurnId::new(), input)
            .await
            .expect("accept projection fixture");
        let attempt_id = AttemptId::new();
        let planned = vec![
            Item::new(
                1,
                ItemPayload::ToolCall {
                    descriptor_id: "fixture.tool".to_owned(),
                    descriptor_version: "v1".to_owned(),
                    target: "fixture-target".to_owned(),
                    attempt_id: Some(attempt_id),
                    status: Some(ExecutionStatus::Running),
                    version: Some(2),
                },
            ),
            Item::new(
                1,
                ItemPayload::ToolResult {
                    attempt_id: Some(attempt_id),
                    status: ExecutionStatus::Succeeded,
                    code: None,
                    effect_state: Some(ToolEffectState::Started),
                    output_bytes: 2,
                    output_digest: Some(crate::application::output_digest(b"ok")),
                    version: Some(3),
                },
            ),
        ];

        let committed = append(&pool, &accepted, planned.clone())
            .await
            .expect("projection transaction commits before acknowledgement is lost");
        let reconciled = super::super::super::commit_reconciliation::appended_projection(
            &pool, &accepted, planned,
        )
        .await
        .expect("reconcile committed projection");

        assert_eq!(reconciled, Some(committed));
        pool.close().await;
    }
}
