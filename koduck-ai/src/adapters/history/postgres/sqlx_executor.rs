// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `SQLx`-backed implementation of the canonical `PostgreSQL` transaction boundary.

use std::time::Duration;

use sqlx::{PgPool, Row};
use tokio::runtime::Handle;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::application::{AcceptedTurn, AppendPolicy, HistoryError, NewItem, TurnCommand};
use crate::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
};

use super::commit_reconciliation;
use super::payload_codec::{encode_payload, row_to_item};
use super::settle_commit_attempt;
use super::{LeaseKey, LeaseTiming, PostgresExecutor, ReconcileOutcome, RecoveryOutcome};

mod interruption_ownership;
mod projection_batch;
/// Production `PostgreSQL` executor using one `SQLx` pool and its owning Tokio runtime.
#[derive(Clone)]
pub struct SqlxPostgresExecutor {
    pool: PgPool,
    runtime: Handle,
}

impl SqlxPostgresExecutor {
    /// Creates an executor whose synchronous port calls drive `SQLx` on `runtime`.
    #[must_use]
    pub const fn new(pool: PgPool, runtime: Handle) -> Self {
        Self { pool, runtime }
    }

    fn wait<T>(
        &self,
        operation: impl Future<Output = Result<T, HistoryError>>,
    ) -> Result<T, HistoryError> {
        self.wait_with_deadline(AppendPolicy::cand_1().deadline(), operation)
    }

    /// Drives one database attempt until its caller-owned deadline expires.
    pub(super) fn wait_with_deadline<T>(
        &self,
        deadline: Duration,
        operation: impl Future<Output = Result<T, HistoryError>>,
    ) -> Result<T, HistoryError> {
        self.runtime.block_on(async {
            tokio::time::timeout(deadline, operation)
                .await
                .map_err(|_| HistoryError::Unavailable)?
        })
    }

    async fn interruption_requested_async(
        &self,
        turn: &AcceptedTurn,
    ) -> Result<bool, HistoryError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT t.interrupt_requested FROM turns t \
             JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
             WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3 \
             AND l.generation = $4 AND NOT l.fenced AND t.status = 'started'",
        )
        .bind(turn.tenant_id.as_str())
        .bind(turn.thread_id.as_uuid())
        .bind(turn.turn_id.as_uuid())
        .bind(generation_i64(turn.generation)?)
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?
        .ok_or(HistoryError::Fenced)
    }

    async fn prior_thread_items_async(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        let rows = sqlx::query(
            "SELECT turn_items.item_id, turn_items.sequence, turn_items.item_type, \
             turn_items.payload FROM turn_items JOIN turns \
             ON turns.tenant_id = turn_items.tenant_id \
             AND turns.thread_id = turn_items.thread_id \
             AND turns.turn_id = turn_items.turn_id JOIN threads \
             ON threads.tenant_id = turn_items.tenant_id \
             AND threads.thread_id = turn_items.thread_id \
             WHERE turn_items.tenant_id = $1 AND turn_items.thread_id = $2 \
             AND threads.subject_id = $3 ORDER BY turns.created_at, \
             turn_items.turn_id, turn_items.sequence LIMIT $4",
        )
        .bind(trust.tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(trust.subject_id.as_str())
        .bind(commit_reconciliation::MAX_PROVIDER_HISTORY_QUERY_ROWS)
        .fetch(&self.pool);
        tokio::pin!(rows);
        let mut history = Vec::new();
        let mut payload_bytes = 0_usize;
        while let Some(row) = rows.next().await {
            commit_reconciliation::push_bounded_history(
                &mut history,
                &mut payload_bytes,
                row_to_item(&row.map_err(unavailable)?)?,
            )?;
        }
        if history.is_empty() {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id = $1 \
                 AND thread_id = $2 AND threads.subject_id = $3)",
            )
            .bind(trust.tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .bind(trust.subject_id.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(unavailable)?;
            if !exists {
                return Err(HistoryError::NotFound);
            }
        }
        Ok(history)
    }

    async fn accept_initial_with_identity_async(
        &self,
        command: &TurnCommand,
        thread_id: ThreadId,
        turn_id: TurnId,
        input: Item,
    ) -> Result<AcceptedTurn, HistoryError> {
        let tenant_id = command.trust.tenant_id.clone();
        let generation = LeaseGeneration::initial();
        let (_, payload, _, _) = encode_payload(&input.payload);
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        commit_reconciliation::lock_operation(&mut transaction, input.item_id.as_uuid()).await?;
        sqlx::query(
            "INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3) \
             ON CONFLICT (tenant_id, thread_id) DO NOTHING",
        )
        .bind(tenant_id.as_str())
        .bind(command.trust.subject_id.as_str())
        .bind(thread_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        let owns_thread = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id = $1 \
             AND subject_id = $2 AND thread_id = $3)",
        )
        .bind(tenant_id.as_str())
        .bind(command.trust.subject_id.as_str())
        .bind(thread_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
        if !owns_thread {
            return Err(HistoryError::NotFound);
        }
        sqlx::query(
            "INSERT INTO turns \
             (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'started', 2)",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        sqlx::query(
            "INSERT INTO turn_items \
             (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, is_terminal) \
             VALUES ($1, $2, $3, 1, $4, 'user_message', $5, FALSE)",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(input.item_id.as_uuid())
        .bind(payload)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        sqlx::query(
            "INSERT INTO turn_leases \
             (tenant_id, thread_id, turn_id, generation, renewed_at, expires_at) \
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, \
                     CURRENT_TIMESTAMP + INTERVAL '20 seconds')",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(generation_i64(generation)?)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(AcceptedTurn::new(
            tenant_id, thread_id, turn_id, generation, input,
        ))
    }

    async fn append_async(
        &self,
        turn: &AcceptedTurn,
        new_item: NewItem,
        item: Item,
    ) -> Result<Item, HistoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        commit_reconciliation::lock_operation(&mut transaction, item.item_id.as_uuid()).await?;
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
        if is_terminal_status(&status) {
            return Err(HistoryError::AlreadyTerminal);
        }
        if fenced || status != "started" {
            return Err(HistoryError::Fenced);
        }
        let interrupt_requested: bool = ownership
            .try_get("interrupt_requested")
            .map_err(unavailable)?;
        let interrupting: bool = ownership.try_get("interrupting").map_err(unavailable)?;
        if interrupting {
            return Err(HistoryError::Fenced);
        }
        let new_item = if interrupt_requested {
            NewItem::Terminal(TerminalOutcome::Interrupted)
        } else {
            new_item
        };
        let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
        let sequence = u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?;
        let mut item = item;
        item.sequence = sequence;
        item.payload = new_item.into_payload();
        insert_item(
            &mut transaction,
            &turn.tenant_id,
            turn.thread_id,
            turn.turn_id,
            &item,
        )
        .await?;
        let (_, _, _, terminal_status) = encode_payload(&item.payload);
        if let Some(terminal_status) = terminal_status {
            sqlx::query(
                "UPDATE turns SET next_sequence = next_sequence + 1, status = $5 \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
                 AND next_sequence = $4",
            )
            .bind(turn.tenant_id.as_str())
            .bind(turn.thread_id.as_uuid())
            .bind(turn.turn_id.as_uuid())
            .bind(sequence_i64(item.sequence)?)
            .bind(terminal_status)
            .execute(&mut *transaction)
            .await
            .map_err(unavailable)?;
        } else {
            sqlx::query(
                "UPDATE turns SET next_sequence = next_sequence + 1 \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
                 AND next_sequence = $4",
            )
            .bind(turn.tenant_id.as_str())
            .bind(turn.thread_id.as_uuid())
            .bind(turn.turn_id.as_uuid())
            .bind(sequence_i64(item.sequence)?)
            .execute(&mut *transaction)
            .await
            .map_err(unavailable)?;
        }
        transaction.commit().await.map_err(unavailable)?;
        Ok(item)
    }

    async fn append_tool_projection_async(
        &self,
        turn: &AcceptedTurn,
        items: Vec<Item>,
    ) -> Result<Vec<Item>, HistoryError> {
        projection_batch::append(&self.pool, turn, items).await
    }

    async fn replay_async(
        &self,
        tenant_id: &TenantId,
        turn_id: TurnId,
    ) -> Result<Vec<Item>, HistoryError> {
        let rows = sqlx::query(
            "SELECT item_id, sequence, item_type, payload FROM turn_items \
             WHERE tenant_id = $1 AND turn_id = $2 ORDER BY sequence",
        )
        .bind(tenant_id.as_str())
        .bind(turn_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        if rows.is_empty() {
            return Err(HistoryError::NotFound);
        }
        rows.iter().map(row_to_item).collect()
    }

    async fn recover_failed_async(
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
        let terminal;
        let terminal_status;
        if interrupt_requested {
            terminal = TerminalOutcome::Interrupted;
            terminal_status = "interrupted";
        } else {
            terminal = TerminalOutcome::Failed {
                code: "DURABILITY_UNAVAILABLE".to_owned(),
            };
            terminal_status = "failed";
        }
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

    async fn renew_lease_async(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        let result = sqlx::query(
            "UPDATE turn_leases l SET \
             renewed_at = to_timestamp($5::double precision / 1000.0), \
             expires_at = to_timestamp($5::double precision / 1000.0) + INTERVAL '20 seconds' \
             FROM turns t WHERE l.tenant_id = $1 AND l.thread_id = $2 \
             AND l.turn_id = $3 AND l.generation = $4 AND NOT l.fenced \
             AND t.tenant_id = l.tenant_id AND t.thread_id = l.thread_id \
             AND t.turn_id = l.turn_id AND t.status = 'started' \
             AND NOT t.interrupting",
        )
        .bind(key.tenant_id.as_str())
        .bind(key.thread_id.as_uuid())
        .bind(key.turn_id.as_uuid())
        .bind(generation_i64(key.generation)?)
        .bind(milliseconds_i64(now_ms)?)
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        (result.rows_affected() == 1)
            .then_some(())
            .ok_or(HistoryError::Fenced)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one transaction must lock ownership, terminalize recovered D-6/D-7 state, append the Turn terminal, and fence the lease atomically"
    )]
    async fn reconcile_expired_async(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let ownership = sqlx::query(
            "SELECT t.status, t.next_sequence, t.interrupt_requested, t.interrupting, l.fenced, \
             (EXTRACT(EPOCH FROM l.renewed_at) * 1000)::BIGINT AS renewed_ms \
             FROM turns t JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
             WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3 \
             AND l.generation = $4 FOR UPDATE",
        )
        .bind(key.tenant_id.as_str())
        .bind(key.thread_id.as_uuid())
        .bind(key.turn_id.as_uuid())
        .bind(generation_i64(key.generation)?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
        .ok_or(HistoryError::Fenced)?;
        let status: String = ownership.try_get("status").map_err(unavailable)?;
        if is_terminal_status(&status) {
            return Err(HistoryError::AlreadyTerminal);
        }
        let fenced: bool = ownership.try_get("fenced").map_err(unavailable)?;
        if fenced {
            return Err(HistoryError::Fenced);
        }
        let renewed_ms: i64 = ownership.try_get("renewed_ms").map_err(unavailable)?;
        let renewed_ms = u64::try_from(renewed_ms).map_err(|_| HistoryError::Unavailable)?;
        if now_ms < renewed_ms.saturating_add(timing.reconcile_after_ms()) {
            return Ok(ReconcileOutcome::TooEarly);
        }
        let interrupt_requested: bool = ownership
            .try_get("interrupt_requested")
            .map_err(unavailable)?;
        let interrupting: bool = ownership.try_get("interrupting").map_err(unavailable)?;
        let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
        // The interrupting flag is the durable pre-terminal barrier. If the
        // requesting process dies or loses its lease after committing that
        // barrier but before it writes the Turn terminal, expiry recovery
        // must finish the same authenticated interruption rather than leave
        // the Turn permanently fenced.
        // Every expiry terminal fences this lease permanently. Close any D-7
        // still owned by the Turn before doing so, because no later reconciler
        // can reach an active attempt beneath a terminal Turn. A recovered
        // interruption also expires its remaining requested D-6 approvals in
        // this transaction: the barrier makes them permanently unconsumable.
        super::attempt_recovery::close_active_attempts(&mut transaction, key, now_ms).await?;
        let recovered_interruption = interrupt_requested || interrupting;
        if recovered_interruption {
            super::attempt_recovery::expire_requested_approvals(&mut transaction, key, now_ms)
                .await?;
        }
        let (terminal, terminal_status, outcome) = if recovered_interruption {
            (
                TerminalOutcome::Interrupted,
                "interrupted",
                ReconcileOutcome::Interrupted,
            )
        } else if status == "recovery-pending" {
            (
                TerminalOutcome::Failed {
                    code: "DURABILITY_UNAVAILABLE".to_owned(),
                },
                "failed",
                ReconcileOutcome::Failed,
            )
        } else {
            (
                TerminalOutcome::Cancelled,
                "cancelled",
                ReconcileOutcome::Cancelled,
            )
        };
        let item = Item::new(
            u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
            ItemPayload::Terminal(terminal),
        );
        insert_item(
            &mut transaction,
            &key.tenant_id,
            key.thread_id,
            key.turn_id,
            &item,
        )
        .await?;
        sqlx::query(
            "UPDATE turns SET status = $5, next_sequence = next_sequence + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND next_sequence = $4 AND status = $6",
        )
        .bind(key.tenant_id.as_str())
        .bind(key.thread_id.as_uuid())
        .bind(key.turn_id.as_uuid())
        .bind(sequence_i64(item.sequence)?)
        .bind(terminal_status)
        .bind(status)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND generation = $4 AND NOT fenced",
        )
        .bind(key.tenant_id.as_str())
        .bind(key.thread_id.as_uuid())
        .bind(key.turn_id.as_uuid())
        .bind(generation_i64(key.generation)?)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(outcome)
    }

    async fn expired_lease_keys_async(
        &self,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<Vec<LeaseKey>, HistoryError> {
        let rows = sqlx::query(
            "SELECT l.tenant_id, l.thread_id, l.turn_id, l.generation \
             FROM turn_leases l JOIN turns t USING (tenant_id, thread_id, turn_id) \
             WHERE t.status IN ('started', 'recovery-pending') AND NOT l.fenced \
             AND (EXTRACT(EPOCH FROM l.renewed_at) * 1000)::BIGINT <= $1 - $2",
        )
        .bind(milliseconds_i64(now_ms)?)
        .bind(milliseconds_i64(timing.reconcile_after_ms())?)
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        rows.iter()
            .map(|row| {
                let tenant_id: String = row.try_get("tenant_id").map_err(unavailable)?;
                let thread_id: Uuid = row.try_get("thread_id").map_err(unavailable)?;
                let turn_id: Uuid = row.try_get("turn_id").map_err(unavailable)?;
                let generation: i64 = row.try_get("generation").map_err(unavailable)?;
                let generation = u64::try_from(generation)
                    .ok()
                    .and_then(LeaseGeneration::from_persisted)
                    .ok_or(HistoryError::Unavailable)?;
                Ok(LeaseKey::new(
                    TenantId::new(tenant_id).map_err(|_| HistoryError::Unavailable)?,
                    ThreadId::from_uuid(thread_id),
                    TurnId::from_uuid(turn_id),
                    generation,
                ))
            })
            .collect()
    }
}

impl PostgresExecutor for SqlxPostgresExecutor {
    fn request_interrupt(&self, trust: &TrustContext, turn_id: TurnId) -> Result<(), HistoryError> {
        self.wait(interruption_ownership::request(&self.pool, trust, turn_id))
    }

    fn interruption_thread(
        &self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<Option<ThreadId>, HistoryError> {
        self.wait(interruption_ownership::resolve(&self.pool, trust, turn_id))
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        self.wait(self.interruption_requested_async(turn))
    }

    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        self.wait(self.prior_thread_items_async(trust, thread_id))
    }

    fn accept_initial(&self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let command = command.clone();
        let thread_id = command.thread_id.unwrap_or_default();
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.runtime.block_on(settle_commit_attempt(
            AppendPolicy::cand_1().deadline(),
            self.accept_initial_with_identity_async(&command, thread_id, turn_id, input.clone()),
            commit_reconciliation::accepted_turn(&self.pool, &command, thread_id, turn_id, input),
        ))
    }

    fn append(&self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let operation_item = Item::new(1, item.clone().into_payload());
        self.runtime.block_on(settle_commit_attempt(
            AppendPolicy::cand_1().deadline(),
            self.append_async(turn, item, operation_item.clone()),
            commit_reconciliation::appended_item(
                &self.pool,
                turn,
                operation_item.item_id.as_uuid(),
            ),
        ))
    }

    fn append_tool_projection(
        &self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        let items = items
            .into_iter()
            .map(|item| Item::new(1, item.into_payload()))
            .collect::<Vec<_>>();
        let reconciliation_items = items.clone();
        self.runtime.block_on(settle_commit_attempt(
            AppendPolicy::cand_1().deadline(),
            self.append_tool_projection_async(turn, items),
            commit_reconciliation::appended_projection(&self.pool, turn, reconciliation_items),
        ))
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.wait(self.replay_async(tenant_id, turn_id))
    }

    fn renew_lease(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        self.wait_with_deadline(
            AppendPolicy::cand_1().deadline(),
            self.renew_lease_async(key, now_ms),
        )
    }

    fn reconcile_expired(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError> {
        self.wait_with_deadline(
            AppendPolicy::cand_1().deadline(),
            self.reconcile_expired_async(key, now_ms, timing),
        )
    }

    fn expired_lease_keys(
        &self,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<Vec<LeaseKey>, HistoryError> {
        self.wait_with_deadline(
            AppendPolicy::cand_1().deadline(),
            self.expired_lease_keys_async(now_ms, timing),
        )
    }

    fn recover_failed(
        &self,
        turn: &AcceptedTurn,
        timing: LeaseTiming,
    ) -> Result<RecoveryOutcome, HistoryError> {
        self.wait_with_deadline(
            AppendPolicy::cand_1().deadline(),
            self.recover_failed_async(turn, timing),
        )
    }

    fn recover_failed_with_deadline(
        &self,
        turn: &AcceptedTurn,
        timing: LeaseTiming,
        deadline: Duration,
    ) -> Result<RecoveryOutcome, HistoryError> {
        self.wait_with_deadline(deadline, self.recover_failed_async(turn, timing))
    }
}

async fn insert_item(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    thread_id: ThreadId,
    turn_id: TurnId,
    item: &Item,
) -> Result<(), HistoryError> {
    let (item_type, payload, is_terminal, _) = encode_payload(&item.payload);
    sqlx::query(
        "INSERT INTO turn_items \
         (tenant_id, thread_id, turn_id, sequence, item_id, item_type, payload, is_terminal) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(tenant_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(sequence_i64(item.sequence)?)
    .bind(item.item_id.as_uuid())
    .bind(item_type)
    .bind(payload)
    .bind(is_terminal)
    .execute(&mut **transaction)
    .await
    .map_err(unavailable)?;
    Ok(())
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "interrupted" | "cancelled")
}

fn generation_i64(generation: LeaseGeneration) -> Result<i64, HistoryError> {
    i64::try_from(generation.get()).map_err(|_| HistoryError::Unavailable)
}

fn sequence_i64(sequence: u64) -> Result<i64, HistoryError> {
    i64::try_from(sequence).map_err(|_| HistoryError::Unavailable)
}

pub(super) fn milliseconds_i64(milliseconds: u64) -> Result<i64, HistoryError> {
    i64::try_from(milliseconds).map_err(|_| HistoryError::Unavailable)
}

pub(super) fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}
