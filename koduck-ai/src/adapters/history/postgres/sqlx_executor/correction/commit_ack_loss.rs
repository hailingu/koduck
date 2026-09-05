// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-4: the deterministic commit-acknowledgement-loss commit arm against
//! a real `PostgreSQL` (CA-07 and CA-08). The write invocation drives the
//! production `SqlxPostgresExecutor` correction port end to end; the
//! test-only seam drops the acknowledgement after the commit is handed to
//! the driver, so the bounded reconciliation must observe the committed
//! exact match through the identity-lock handoff.

use std::sync::atomic::Ordering;

use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

use crate::adapters::history::postgres::SqlxPostgresExecutor;
use crate::application::{CorrectionCommand, CorrectionStore};
use crate::domain::{ItemId, TenantId, ThreadId, TrustContext, TurnId};

use super::DROP_COMMIT_ACK;

/// Disarms the commit-ack-loss seam even when an assertion fails, so other
/// library tests never observe the faulted mode.
struct AckLossGuard;

impl Drop for AckLossGuard {
    fn drop(&mut self) {
        DROP_COMMIT_ACK.store(false, Ordering::Release);
    }
}

#[test]
fn commit_ack_loss_is_reconciled_to_the_committed_exact_match() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());

    let tenant = TenantId::new(format!("cand11-ack-loss-{}", Uuid::new_v4())).expect("tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let input_id = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let identity = ItemId::new();
    let command = correction_command(&tenant, thread, turn, identity, input_id);

    DROP_COMMIT_ACK.store(true, Ordering::Release);
    let guard = AckLossGuard;
    let item = correct(&runtime, &pool, command);
    std::mem::drop(guard);

    // The reconciler waits behind the still-committing writer's identity
    // lock and then observes the committed exact match.
    assert_eq!(item.item_id, identity);
    assert_eq!(item.sequence, 2);
    assert_durable_state(&runtime, &pool, &tenant, thread, turn, 2, 3);

    // The later exact retry deduplicates without any new write.
    let retried = correct(
        &runtime,
        &pool,
        correction_command(&tenant, thread, turn, identity, input_id),
    );
    assert_eq!(retried, item);
    assert_durable_state(&runtime, &pool, &tenant, thread, turn, 2, 3);
    runtime.block_on(pool.close());
}

fn connected_pool() -> (tokio::runtime::Runtime, PgPool) {
    let Ok(database_url) = std::env::var("KODUCK_AI_TEST_DATABASE_URL") else {
        panic!(
            "KODUCK_AI_TEST_DATABASE_URL must point at the isolated migrated \
             PostgreSQL database"
        );
    };
    // The synchronous port drives the database on its own runtime; the test
    // thread calls it from outside that runtime.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("the commit-ack-loss test runtime");
    let pool = runtime.block_on(async {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .expect("connect to the disposable test PostgreSQL");
        crate::test_migrations::ensure(&pool).await;
        pool
    });
    (runtime, pool)
}

fn correction_command(
    tenant: &TenantId,
    thread: ThreadId,
    turn: TurnId,
    identity: ItemId,
    input_id: ItemId,
) -> CorrectionCommand {
    CorrectionCommand::new(
        TrustContext::new(tenant.clone(), "subject-a").expect("trust"),
        thread,
        turn,
        identity,
        input_id,
        "committed before the acknowledgement vanished",
    )
    .expect("valid command")
}

fn correct(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    command: CorrectionCommand,
) -> crate::domain::Item {
    CorrectionStore::correct(
        &SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone()),
        command,
    )
    .expect("the reconciliation resolves the committed exact match")
}

fn assert_durable_state(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    tenant: &TenantId,
    thread: ThreadId,
    turn: TurnId,
    rows: i64,
    next_sequence: i64,
) {
    let (stored_rows, counter): (i64, i64) = runtime.block_on(async {
        sqlx::query_as(
            "SELECT \
         (SELECT count(*) FROM turn_items WHERE tenant_id = $1 \
          AND thread_id = $2 AND turn_id = $3), \
         (SELECT next_sequence FROM turns WHERE tenant_id = $1 \
          AND thread_id = $2 AND turn_id = $3)",
        )
        .bind(tenant.as_str())
        .bind(thread.as_uuid())
        .bind(turn.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the durable state")
    });
    assert_eq!(stored_rows, rows, "exactly one correction row may exist");
    assert_eq!(counter, next_sequence, "the counter advanced exactly once");
}

async fn seed_completed_turn(pool: &PgPool, tenant: &TenantId, thread: &ThreadId, turn: &TurnId) {
    sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
        .bind(tenant.as_str())
        .bind("subject-a")
        .bind(thread.as_uuid())
        .execute(pool)
        .await
        .expect("seed thread");
    sqlx::query(
        "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
         VALUES ($1, $2, $3, 'completed', 2)",
    )
    .bind(tenant.as_str())
    .bind(thread.as_uuid())
    .bind(turn.as_uuid())
    .execute(pool)
    .await
    .expect("seed turn");
    sqlx::query(
        "INSERT INTO turn_leases (tenant_id, thread_id, turn_id, generation, \
         renewed_at, expires_at, fenced) \
         VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP, \
                 CURRENT_TIMESTAMP + INTERVAL '1 hour', FALSE)",
    )
    .bind(tenant.as_str())
    .bind(thread.as_uuid())
    .bind(turn.as_uuid())
    .execute(pool)
    .await
    .expect("seed lease");
    sqlx::query(
        "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
         item_type, payload, is_terminal, corrects_item_id) \
         VALUES ($1, $2, $3, 1, $4, 'user_message', '{\"content\":\"original\"}', FALSE, NULL)",
    )
    .bind(tenant.as_str())
    .bind(thread.as_uuid())
    .bind(turn.as_uuid())
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .expect("seed input item");
}

async fn seeded_input_id(
    pool: &PgPool,
    tenant: &TenantId,
    thread: &ThreadId,
    turn: &TurnId,
) -> ItemId {
    let id: Uuid = sqlx::query_scalar(
        "SELECT item_id FROM turn_items WHERE tenant_id = $1 \
         AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
    )
    .bind(tenant.as_str())
    .bind(thread.as_uuid())
    .bind(turn.as_uuid())
    .fetch_one(pool)
    .await
    .expect("read the seeded input identity");
    ItemId::from_uuid(id)
}
