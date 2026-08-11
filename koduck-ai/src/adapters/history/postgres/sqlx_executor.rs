// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `SQLx`-backed implementation of the canonical `PostgreSQL` transaction boundary.

use std::future::Future;

use serde_json::{Value, json};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use tokio::runtime::Handle;
use uuid::Uuid;

use crate::application::{AcceptedTurn, HistoryError, NewItem, TurnCommand};
use crate::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    Usage,
};

use super::{LeaseKey, LeaseTiming, PostgresExecutor, ReconcileOutcome};

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
        self.runtime.block_on(operation)
    }

    async fn request_interrupt_async(
        &self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        let result = sqlx::query(
            "UPDATE turns SET interrupt_requested = TRUE \
             WHERE tenant_id = $1 AND turn_id = $2 AND status = 'started'",
        )
        .bind(trust.tenant_id.as_str())
        .bind(turn_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(unavailable)?;
        if result.rows_affected() == 1 {
            return Ok(());
        }
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM turns WHERE tenant_id = $1 AND turn_id = $2",
        )
        .bind(trust.tenant_id.as_str())
        .bind(turn_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(unavailable)?;
        match status.as_deref() {
            None => Err(HistoryError::NotFound),
            Some("completed" | "failed" | "interrupted" | "cancelled") => {
                Err(HistoryError::AlreadyTerminal)
            }
            Some(_) => Err(HistoryError::Fenced),
        }
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
        tenant_id: &TenantId,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        let rows = sqlx::query(
            "SELECT item_id, sequence, item_type, payload FROM turn_items \
             WHERE tenant_id = $1 AND thread_id = $2 \
             ORDER BY created_at, turn_id, sequence",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(unavailable)?;
        if rows.is_empty() {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id = $1 AND thread_id = $2)",
            )
            .bind(tenant_id.as_str())
            .bind(thread_id.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(unavailable)?;
            if !exists {
                return Err(HistoryError::NotFound);
            }
        }
        rows.iter().map(row_to_item).collect()
    }

    async fn accept_initial_async(
        &self,
        command: &TurnCommand,
    ) -> Result<AcceptedTurn, HistoryError> {
        let tenant_id = command.trust.tenant_id.clone();
        let thread_id = command.thread_id.unwrap_or_default();
        let turn_id = TurnId::new();
        let generation = LeaseGeneration::initial();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        let (_, payload, _, _) = encode_payload(&input.payload);
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        sqlx::query(
            "INSERT INTO threads (tenant_id, thread_id) VALUES ($1, $2) \
             ON CONFLICT (tenant_id, thread_id) DO NOTHING",
        )
        .bind(tenant_id.as_str())
        .bind(thread_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
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
    ) -> Result<Item, HistoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let ownership = sqlx::query(
            "SELECT t.status, t.next_sequence, l.fenced FROM turns t \
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
        let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
        let sequence = u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?;
        let item = Item::new(sequence, new_item.into_payload());
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

    async fn renew_lease_async(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        let result = sqlx::query(
            "UPDATE turn_leases l SET \
             renewed_at = to_timestamp($5::double precision / 1000.0), \
             expires_at = to_timestamp($5::double precision / 1000.0) + INTERVAL '20 seconds' \
             FROM turns t WHERE l.tenant_id = $1 AND l.thread_id = $2 \
             AND l.turn_id = $3 AND l.generation = $4 AND NOT l.fenced \
             AND t.tenant_id = l.tenant_id AND t.thread_id = l.thread_id \
             AND t.turn_id = l.turn_id AND t.status = 'started'",
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

    async fn reconcile_expired_async(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        let ownership = sqlx::query(
            "SELECT t.status, t.next_sequence, l.fenced, \
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
        let sequence: i64 = ownership.try_get("next_sequence").map_err(unavailable)?;
        let item = Item::new(
            u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
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
            "UPDATE turns SET status = 'cancelled', next_sequence = next_sequence + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND next_sequence = $4",
        )
        .bind(key.tenant_id.as_str())
        .bind(key.thread_id.as_uuid())
        .bind(key.turn_id.as_uuid())
        .bind(sequence_i64(item.sequence)?)
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
        Ok(ReconcileOutcome::Cancelled)
    }
}

impl PostgresExecutor for SqlxPostgresExecutor {
    fn request_interrupt(&self, trust: &TrustContext, turn_id: TurnId) -> Result<(), HistoryError> {
        self.wait(self.request_interrupt_async(trust, turn_id))
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        self.wait(self.interruption_requested_async(turn))
    }

    fn prior_thread_items(
        &self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        self.wait(self.prior_thread_items_async(tenant_id, thread_id))
    }

    fn accept_initial(&self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        self.wait(self.accept_initial_async(command))
    }

    fn append(&self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.wait(self.append_async(turn, item))
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.wait(self.replay_async(tenant_id, turn_id))
    }

    fn renew_lease(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        self.wait(self.renew_lease_async(key, now_ms))
    }

    fn reconcile_expired(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError> {
        self.wait(self.reconcile_expired_async(key, now_ms, timing))
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

fn encode_payload(payload: &ItemPayload) -> (&'static str, Value, bool, Option<&'static str>) {
    match payload {
        ItemPayload::UserMessage { content } => {
            ("user_message", json!({ "content": content }), false, None)
        }
        ItemPayload::AgentMessageDelta { content } => (
            "agent_message_delta",
            json!({ "content": content }),
            false,
            None,
        ),
        ItemPayload::Usage(usage) => ("usage", usage_json(*usage), false, None),
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }) => {
            ("completed", usage_json(*usage), true, Some("completed"))
        }
        ItemPayload::Terminal(TerminalOutcome::Failed { code }) => {
            ("failed", json!({ "code": code }), true, Some("failed"))
        }
        ItemPayload::Terminal(TerminalOutcome::Interrupted) => {
            ("interrupted", json!({}), true, Some("interrupted"))
        }
        ItemPayload::Terminal(TerminalOutcome::Cancelled) => {
            ("cancelled", json!({}), true, Some("cancelled"))
        }
    }
}

fn usage_json(usage: Usage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.total_tokens,
    })
}

fn row_to_item(row: &PgRow) -> Result<Item, HistoryError> {
    let item_id: Uuid = row.try_get("item_id").map_err(unavailable)?;
    let sequence: i64 = row.try_get("sequence").map_err(unavailable)?;
    let item_type: String = row.try_get("item_type").map_err(unavailable)?;
    let payload: Value = row.try_get("payload").map_err(unavailable)?;
    let payload = decode_payload(&item_type, &payload)?;
    Ok(Item {
        item_id: crate::domain::ItemId::from_uuid(item_id),
        sequence: u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
        payload,
    })
}

fn decode_payload(item_type: &str, payload: &Value) -> Result<ItemPayload, HistoryError> {
    let text = || {
        payload
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(HistoryError::Unavailable)
    };
    match item_type {
        "user_message" => Ok(ItemPayload::UserMessage { content: text()? }),
        "agent_message_delta" => Ok(ItemPayload::AgentMessageDelta { content: text()? }),
        "usage" => Ok(ItemPayload::Usage(decode_usage(payload)?)),
        "completed" => Ok(ItemPayload::Terminal(TerminalOutcome::Completed {
            usage: decode_usage(payload)?,
        })),
        "failed" => Ok(ItemPayload::Terminal(TerminalOutcome::Failed {
            code: payload
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(HistoryError::Unavailable)?,
        })),
        "interrupted" => Ok(ItemPayload::Terminal(TerminalOutcome::Interrupted)),
        "cancelled" => Ok(ItemPayload::Terminal(TerminalOutcome::Cancelled)),
        _ => Err(HistoryError::Unavailable),
    }
}

fn decode_usage(payload: &Value) -> Result<Usage, HistoryError> {
    let input = payload
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let output = payload
        .get("output_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let usage = Usage::new(input, output).map_err(|_| HistoryError::Unavailable)?;
    (payload.get("total_tokens").and_then(Value::as_u64) == Some(usage.total_tokens))
        .then_some(usage)
        .ok_or(HistoryError::Unavailable)
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

fn milliseconds_i64(milliseconds: u64) -> Result<i64, HistoryError> {
    i64::try_from(milliseconds).map_err(|_| HistoryError::Unavailable)
}

fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}
