// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! CA-04/CA-05: exact retry and reconciliation reject a terminal Correction
//! even when restored history lacks the production shape constraint.

use sqlx::PgPool;
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::adapters::history::postgres::SqlxPostgresExecutor;
use crate::application::{CorrectionCommand, CorrectionError, CorrectionStore};
use crate::domain::{Item, ItemId, TenantId, ThreadId, TurnId};

use super::commit_ack_loss::{
    assert_durable_state, connected_pool, correction_command, seed_completed_turn, seeded_input_id,
};
use super::payload_read_race::connect_scoped_reader;

/// Covers the flag's corruption, identity precedence, and recovery with two
/// copied rows while leaving production constraint protection intact.
#[test]
fn exact_retry_rejects_a_terminal_correction() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    let tenant =
        TenantId::new(format!("cand11-retry-terminal-{}", Uuid::new_v4())).expect("fixture tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let root = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let command = correction_command(&tenant, thread, turn, ItemId::new(), root);
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let original = executor.correct(command.clone()).expect("admit correction");
    let schema = format!("cand11_terminal_copy_{}", Uuid::new_v4().simple());
    let corrupt = runtime.block_on(copy_items(&pool, &schema, &command));
    assert_retry_outcome(&runtime, &corrupt, &command, &Ok(original.clone()), false);

    runtime.block_on(set_terminal_flag(&corrupt, &command, true));
    assert_retry_outcome(
        &runtime,
        &corrupt,
        &command,
        &Err(CorrectionError::CorruptHistory),
        true,
    );
    let content_drift = CorrectionCommand::new(
        command.trust().clone(),
        thread,
        turn,
        command.item_id(),
        root,
        "different replacement",
    )
    .expect("valid content mismatch");
    let predecessor_drift =
        correction_command(&tenant, thread, turn, command.item_id(), ItemId::new());
    for mismatch in [content_drift, predecessor_drift] {
        assert_retry_outcome(
            &runtime,
            &corrupt,
            &mismatch,
            &Err(CorrectionError::IdentityConflict),
            true,
        );
    }
    runtime.block_on(set_terminal_flag(&corrupt, &command, false));
    assert_retry_outcome(&runtime, &corrupt, &command, &Ok(original), false);
    runtime.block_on(production_constraint_rejects_terminal(&pool, &command));
    runtime.block_on(remove_copy(&pool, &corrupt, &schema));
    runtime.block_on(pool.close());
}

/// Uses the existing corrupt-schema strategy with a private table copy; LIKE
/// retains column types/defaults but does not copy the correction CHECK rule.
async fn copy_items(pool: &PgPool, schema: &str, command: &CorrectionCommand) -> PgPool {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; \
         CREATE TABLE {schema}.turn_items (LIKE public.turn_items INCLUDING DEFAULTS)"
    )))
    .execute(pool)
    .await
    .expect("create private correction-shape fixture");
    let reader = connect_scoped_reader(schema).await;
    sqlx::query(
        "INSERT INTO turn_items SELECT * FROM public.turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .execute(&reader)
    .await
    .expect("copy only this fixture's two rows");
    reader
}

/// Changes only the private copy to represent corruption or its repair.
async fn set_terminal_flag(pool: &PgPool, command: &CorrectionCommand, terminal: bool) {
    sqlx::query("UPDATE turn_items SET is_terminal = $3 WHERE tenant_id = $1 AND item_id = $2")
        .bind(command.trust().tenant_id.as_str())
        .bind(command.item_id().as_uuid())
        .bind(terminal)
        .execute(pool)
        .await
        .expect("set the copied Correction flag");
}

/// Verifies both production entry points preserve the exact fixture state.
fn assert_retry_outcome(
    runtime: &Runtime,
    pool: &PgPool,
    command: &CorrectionCommand,
    expected: &Result<Item, CorrectionError>,
    terminal: bool,
) {
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    assert_eq!(&executor.correct(command.clone()), expected, "exact retry");
    assert_eq!(
        &runtime
            .block_on(super::reconcile_async(pool, command.clone()))
            .map(|item| item.expect("the fixture correction exists")),
        expected,
        "read-only reconciliation"
    );
    assert_durable_state(
        runtime,
        pool,
        &command.trust().tenant_id,
        command.thread_id(),
        command.turn_id(),
        2,
        3,
    );
    assert_eq!(runtime.block_on(terminal_flag(pool, command)), terminal);
}

/// Reads the persisted flag so rejection cannot silently repair corrupt data.
async fn terminal_flag(pool: &PgPool, command: &CorrectionCommand) -> bool {
    sqlx::query_scalar("SELECT is_terminal FROM turn_items WHERE tenant_id = $1 AND item_id = $2")
        .bind(command.trust().tenant_id.as_str())
        .bind(command.item_id().as_uuid())
        .fetch_one(pool)
        .await
        .expect("read persisted Correction flag")
}

/// CA-05's normal shape protection still rejects this same mutation with
/// SQLSTATE `23514` (`check_violation`) on the production migrated table.
async fn production_constraint_rejects_terminal(pool: &PgPool, command: &CorrectionCommand) {
    let error = sqlx::query(
        "UPDATE turn_items SET is_terminal = TRUE WHERE tenant_id = $1 AND item_id = $2",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.item_id().as_uuid())
    .execute(pool)
    .await
    .expect_err("the production shape constraint rejects a terminal Correction");
    assert!(
        error
            .as_database_error()
            .is_some_and(|error| error.code().as_deref() == Some("23514"))
    );
    assert!(!terminal_flag(pool, command).await);
}

/// Closes the fixture reader and removes only its generated private schema.
async fn remove_copy(pool: &PgPool, reader: &PgPool, schema: &str) {
    reader.close().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(pool)
        .await
        .expect("remove private correction-shape fixture");
}
