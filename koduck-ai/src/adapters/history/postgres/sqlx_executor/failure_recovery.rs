// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Conditional terminal recovery after a foreground durability failure.

use sqlx::Row;

use crate::application::{AcceptedTurn, HistoryError};
use crate::domain::{Item, ItemPayload, TerminalOutcome};

use super::super::{LeaseTiming, RecoveryOutcome, attempt_recovery, unix_time_ms};
use super::{
    LeaseKey, SqlxPostgresExecutor, generation_i64, insert_item, is_terminal_status,
    milliseconds_i64, sequence_i64, unavailable,
};

impl SqlxPostgresExecutor {
    /// Commits the terminal outcome for a Turn that entered durable recovery.
    #[allow(
        clippy::too_many_lines,
        reason = "one transaction must close foreground-recovered D-6/D-7 state, append their audited projections, and terminalize the Turn atomically"
    )]
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
        // A foreground durability recovery can be the final opportunity to
        // close D-6/D-7 beneath this Turn. Keep those transitions, their
        // audits, and their D-3 projections in the same transaction before
        // making the Turn terminal; otherwise a terminal Turn would be
        // excluded from later expiry recovery (ADR-0003 TC-10/TC-14).
        let key = LeaseKey::new(
            turn.tenant_id.clone(),
            turn.thread_id,
            turn.turn_id,
            turn.generation,
        );
        let terminal_at_millis = unix_time_ms();
        let mut recovered_approval_projections = attempt_recovery::cancel_requested_approvals(
            &mut transaction,
            &key,
            terminal_at_millis,
        )
        .await?;
        let Some(mut recovered_attempt_projections) =
            attempt_recovery::close_active_attempts(&mut transaction, &key, terminal_at_millis)
                .await?
        else {
            // Dropping the transaction rolls back any tentative D-6 close:
            // a live D-7 must remain fully reconcilable until its deadline.
            return Ok(RecoveryOutcome::Pending);
        };
        recovered_approval_projections.append(&mut recovered_attempt_projections);
        let mut next_sequence = u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?;
        for projection in &mut recovered_approval_projections {
            projection.sequence = next_sequence;
            insert_item(
                &mut transaction,
                &turn.tenant_id,
                turn.thread_id,
                turn.turn_id,
                projection,
            )
            .await?;
            next_sequence = next_sequence
                .checked_add(1)
                .ok_or(HistoryError::Unavailable)?;
        }
        let item = Item::new(next_sequence, ItemPayload::Terminal(terminal));
        insert_item(
            &mut transaction,
            &turn.tenant_id,
            turn.thread_id,
            turn.turn_id,
            &item,
        )
        .await?;
        let turn_update = sqlx::query(
            "UPDATE turns SET status = $5, next_sequence = $6 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND next_sequence = $4 AND status = $7",
        )
        .bind(turn.tenant_id.as_str())
        .bind(turn.thread_id.as_uuid())
        .bind(turn.turn_id.as_uuid())
        .bind(sequence)
        .bind(terminal_status)
        .bind(sequence_i64(
            item.sequence
                .checked_add(1)
                .ok_or(HistoryError::Unavailable)?,
        )?)
        .bind(status)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        if turn_update.rows_affected() != 1 {
            return Err(HistoryError::Fenced);
        }
        transaction.commit().await.map_err(unavailable)?;
        Ok(RecoveryOutcome::Failed)
    }
}
