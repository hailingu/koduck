// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Durable operation-identity lookups for uncertain commit acknowledgements.

use sqlx::PgPool;
use uuid::Uuid;

use crate::application::{AcceptedTurn, HistoryError, TurnCommand};
use crate::domain::{Item, LeaseGeneration, ThreadId, TurnId};

use super::payload_codec::row_to_item;

const MAX_PROVIDER_HISTORY_BYTES: usize = 1_048_576;
pub(super) const MAX_PROVIDER_HISTORY_ITEMS: usize = 4_096;
pub(super) const MAX_PROVIDER_HISTORY_QUERY_ROWS: i64 = 4_097;

/// Reconstructs initial acceptance when its generated input identity is durable.
pub(super) async fn accepted_turn(
    pool: &PgPool,
    command: &TurnCommand,
    thread_id: ThreadId,
    turn_id: TurnId,
    input: Item,
) -> Result<Option<AcceptedTurn>, HistoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    lock_operation(&mut transaction, input.item_id.as_uuid()).await?;
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM turns t JOIN threads h \
         USING (tenant_id, thread_id) JOIN turn_items i \
         USING (tenant_id, thread_id, turn_id) JOIN turn_leases l \
         USING (tenant_id, thread_id, turn_id) WHERE t.tenant_id = $1 \
         AND h.subject_id = $2 AND t.thread_id = $3 AND t.turn_id = $4 \
         AND i.item_id = $5 AND i.sequence = 1 AND l.generation = 1)",
    )
    .bind(command.trust.tenant_id.as_str())
    .bind(command.trust.subject_id.as_str())
    .bind(thread_id.as_uuid())
    .bind(turn_id.as_uuid())
    .bind(input.item_id.as_uuid())
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    Ok(exists.then(|| {
        AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input,
        )
    }))
}

/// Returns an append only when its preallocated Item identity is durable.
pub(super) async fn appended_item(
    pool: &PgPool,
    turn: &AcceptedTurn,
    item_id: Uuid,
) -> Result<Option<Item>, HistoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    lock_operation(&mut transaction, item_id).await?;
    let row = sqlx::query(
        "SELECT item_id, sequence, item_type, payload FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
         AND item_id = $4",
    )
    .bind(turn.tenant_id.as_str())
    .bind(turn.thread_id.as_uuid())
    .bind(turn.turn_id.as_uuid())
    .bind(item_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    row.as_ref().map(row_to_item).transpose()
}

/// Reconstructs a complete D-3 projection only when every preallocated Item
/// identity from the atomic batch is durably present in its original order.
pub(super) async fn appended_projection(
    pool: &PgPool,
    turn: &AcceptedTurn,
    planned: Vec<Item>,
) -> Result<Option<Vec<Item>>, HistoryError> {
    let Some(operation) = planned.first() else {
        return Err(HistoryError::Unavailable);
    };
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    lock_operation(&mut transaction, operation.item_id.as_uuid()).await?;
    let mut durable = Vec::with_capacity(planned.len());
    let mut missing = 0_usize;
    for expected in &planned {
        let row = sqlx::query(
            "SELECT item_id, sequence, item_type, payload FROM turn_items \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
             AND item_id = $4",
        )
        .bind(turn.tenant_id.as_str())
        .bind(turn.thread_id.as_uuid())
        .bind(turn.turn_id.as_uuid())
        .bind(expected.item_id.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?;
        let Some(row) = row else {
            missing = missing.saturating_add(1);
            continue;
        };
        let item = row_to_item(&row)?;
        if item.item_id != expected.item_id || item.payload != expected.payload {
            return Err(HistoryError::Unavailable);
        }
        durable.push(item);
    }
    if missing == planned.len() {
        return Ok(None);
    }
    if missing != 0
        || durable.windows(2).any(|pair| {
            pair[0]
                .sequence
                .checked_add(1)
                .is_none_or(|next| pair[1].sequence != next)
        })
    {
        return Err(HistoryError::Unavailable);
    }
    Ok(Some(durable))
}

/// Adds one decoded history item without exceeding the aggregate context budget.
pub(super) fn push_bounded_history(
    history: &mut Vec<Item>,
    payload_bytes: &mut usize,
    item: Item,
) -> Result<(), HistoryError> {
    if history.len() >= MAX_PROVIDER_HISTORY_ITEMS {
        return Err(HistoryError::ContextLimit);
    }
    let next_bytes =
        payload_bytes.saturating_add(super::payload_codec::encode_payload(&item.payload).1.len());
    if next_bytes > MAX_PROVIDER_HISTORY_BYTES {
        return Err(HistoryError::ContextLimit);
    }
    *payload_bytes = next_bytes;
    history.push(item);
    Ok(())
}

/// Serializes one write and its timeout reconciliation by stable Item identity.
pub(super) async fn lock_operation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item_id: Uuid,
) -> Result<(), HistoryError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_key(item_id))
        .execute(&mut **transaction)
        .await
        .map_err(unavailable)?;
    Ok(())
}

fn advisory_key(item_id: Uuid) -> i64 {
    let bytes = item_id.as_bytes();
    i64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    use crate::application::HistoryError;
    use crate::domain::{Item, ItemPayload};

    use super::{MAX_PROVIDER_HISTORY_ITEMS, lock_operation, push_bounded_history};

    #[tokio::test]
    async fn reconciliation_waits_for_the_matching_writer_identity() {
        let Ok(database_url) = std::env::var("KODUCK_AI_TEST_DATABASE_URL") else {
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database is available");
        let item_id = Uuid::new_v4();
        let mut writer = pool.begin().await.expect("writer transaction starts");
        lock_operation(&mut writer, item_id)
            .await
            .expect("writer owns the operation identity");

        let waiter_pool = pool.clone();
        let waiter = tokio::spawn(async move {
            let mut reconciliation = waiter_pool
                .begin()
                .await
                .expect("reconciliation transaction starts");
            lock_operation(&mut reconciliation, item_id)
                .await
                .expect("reconciliation eventually owns the operation identity");
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !waiter.is_finished(),
            "reconciliation must not inspect outcome before the writer releases its identity"
        );

        writer.commit().await.expect("writer releases its identity");
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("reconciliation unblocks after writer completion")
            .expect("reconciliation task succeeds");
    }

    #[test]
    fn aggregate_history_over_one_mib_is_rejected_before_provider_construction() {
        let items = [
            Item::new(
                1,
                ItemPayload::UserMessage {
                    content: "a".repeat(700_000),
                },
            ),
            Item::new(
                2,
                ItemPayload::AgentMessageDelta {
                    content: "b".repeat(400_000),
                },
            ),
        ];
        let mut history = Vec::new();
        let mut payload_bytes = 0;
        assert_eq!(
            push_bounded_history(&mut history, &mut payload_bytes, items[0].clone()),
            Ok(())
        );
        assert_eq!(
            push_bounded_history(&mut history, &mut payload_bytes, items[1].clone()),
            Err(HistoryError::ContextLimit)
        );
    }

    #[test]
    fn aggregate_history_over_four_thousand_ninety_six_items_is_rejected() {
        let mut history = Vec::new();
        let mut payload_bytes = 0;

        for sequence in 1..=MAX_PROVIDER_HISTORY_ITEMS {
            push_bounded_history(
                &mut history,
                &mut payload_bytes,
                Item::new(
                    sequence as u64,
                    ItemPayload::AgentMessageDelta {
                        content: String::new(),
                    },
                ),
            )
            .expect("the first 4096 items remain within the count budget");
        }

        let error = push_bounded_history(
            &mut history,
            &mut payload_bytes,
            Item::new(
                (MAX_PROVIDER_HISTORY_ITEMS + 1) as u64,
                ItemPayload::AgentMessageDelta {
                    content: String::new(),
                },
            ),
        )
        .expect_err("the 4097th item must be rejected");

        assert_eq!(error, HistoryError::ContextLimit);
    }
}
