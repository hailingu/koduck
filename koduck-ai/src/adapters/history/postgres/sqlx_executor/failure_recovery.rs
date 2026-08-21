// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Conditional terminal recovery after a foreground durability failure.

use sqlx::Row;

use crate::application::{AcceptedTurn, HistoryError};
use crate::domain::{Item, ItemPayload, TerminalOutcome};

use super::super::{LeaseTiming, RecoveryOutcome};
use super::{
    SqlxPostgresExecutor, generation_i64, insert_item, is_terminal_status, milliseconds_i64,
    sequence_i64, unavailable,
};

impl SqlxPostgresExecutor {
    /// Commits the terminal outcome for a Turn that entered durable recovery.
    pub(super) async fn recover_failed_async(
        &self,
        turn: &AcceptedTurn,
        timing: LeaseTiming,
    ) -> Result<RecoveryOutcome, HistoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let ownership = sqlx::query(
            "SELECT t.status, t.next_sequence, t.interrupt_requested, t.interrupting, l.fenced, \
             (EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - l.renewed_at)) * 1000)::BIGINT \
             <= $5 AS within_window FROM turns t \
             JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
             WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3 \
             AND l.generation = $4 FOR UPDATE",
        )
        .bind(turn.tenant_id.as_str())
        .bind(turn.thread_id.as_uuid())
        .bind(turn.turn_id.as_uuid())
        .bind(generation_i64(turn.generation)?)
        .bind(milliseconds_i64(timing.reconcile_after_ms())?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
        .ok_or(HistoryError::Fenced)?;
        let status: String = ownership.try_get("status").map_err(unavailable)?;
        if is_terminal_status(&status) {
            return Err(HistoryError::AlreadyTerminal);
        }
        let fenced: bool = ownership.try_get("fenced").map_err(unavailable)?;
        let within_window: bool = ownership.try_get("within_window").map_err(unavailable)?;
        if fenced || !within_window {
            return Err(HistoryError::Fenced);
        }
        let interrupt_requested: bool = ownership
            .try_get("interrupt_requested")
            .map_err(unavailable)?;
        let interrupting: bool = ownership.try_get("interrupting").map_err(unavailable)?;
        if interrupting {
            return Err(HistoryError::Fenced);
        }
        if status == "started" && !interrupt_requested {
            sqlx::query(
                "UPDATE turns SET status = 'recovery-pending' \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
                 AND status = 'started'",
            )
            .bind(turn.tenant_id.as_str())
            .bind(turn.thread_id.as_uuid())
            .bind(turn.turn_id.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(unavailable)?;
            transaction.commit().await.map_err(unavailable)?;
            return Ok(RecoveryOutcome::Pending);
        }
        if status != "recovery-pending" && status != "started" {
            return Err(HistoryError::Fenced);
        }
        let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
        let (terminal, terminal_status) = if interrupt_requested {
            (TerminalOutcome::Interrupted, "interrupted")
        } else {
            (
                TerminalOutcome::Failed {
                    code: "DURABILITY_UNAVAILABLE".to_owned(),
                },
                "failed",
            )
        };
        let item = Item::new(
            u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
            ItemPayload::Terminal(terminal),
        );
        insert_item(
            &mut transaction,
            &turn.tenant_id,
            turn.thread_id,
            turn.turn_id,
            &item,
        )
        .await?;
        sqlx::query(
            "UPDATE turns SET status = $5, next_sequence = next_sequence + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND next_sequence = $4 AND status = $6",
        )
        .bind(turn.tenant_id.as_str())
        .bind(turn.thread_id.as_uuid())
        .bind(turn.turn_id.as_uuid())
        .bind(sequence_i64(item.sequence)?)
        .bind(terminal_status)
        .bind(status)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(RecoveryOutcome::Failed)
    }
}
