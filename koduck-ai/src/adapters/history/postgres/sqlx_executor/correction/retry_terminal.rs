// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! CA-03/CA-04/CA-05: private-history fixtures verify that admission, retry,
//! and reconciliation reject malformed Correction flags and successor shape.

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

/// A malformed Correction deeper in the chain blocks new admission and both
/// exact-match paths; repair restores the chain without mutating failed work.
#[test]
fn terminal_correction_ancestor_is_rejected() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    let tenant = TenantId::new(format!("cand11-terminal-ancestor-{}", Uuid::new_v4()))
        .expect("fixture tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let root = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let first = correction_command(&tenant, thread, turn, ItemId::new(), root);
    let public_executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    public_executor
        .correct(first.clone())
        .expect("first correction");
    let second = correction_command(&tenant, thread, turn, ItemId::new(), first.item_id());
    let stored_second = public_executor
        .correct(second.clone())
        .expect("second correction");
    let third = correction_command(&tenant, thread, turn, ItemId::new(), second.item_id());
    let schema = format!("cand11_ancestor_copy_{}", Uuid::new_v4().simple());
    let copied = runtime.block_on(copy_items(&pool, &schema, &second));
    let copied_executor = SqlxPostgresExecutor::new(copied.clone(), runtime.handle().clone());
    assert_existing_result(&runtime, &copied, &second, &Ok(stored_second.clone()));

    runtime.block_on(set_terminal_flag(&copied, &first, true));
    assert_eq!(
        copied_executor.correct(third.clone()),
        Err(CorrectionError::CorruptHistory),
        "fresh correction rejects the terminal ancestor"
    );
    assert_existing_result(
        &runtime,
        &copied,
        &second,
        &Err(CorrectionError::CorruptHistory),
    );
    let content_drift = CorrectionCommand::new(
        second.trust().clone(),
        thread,
        turn,
        second.item_id(),
        first.item_id(),
        "different replacement",
    )
    .expect("valid content mismatch");
    assert_eq!(
        copied_executor.correct(content_drift),
        Err(CorrectionError::IdentityConflict)
    );
    assert_durable_state(&runtime, &copied, &tenant, thread, turn, 3, 4);
    assert!(runtime.block_on(terminal_flag(&copied, &first)));

    runtime.block_on(set_terminal_flag(&copied, &first, false));
    assert_existing_result(&runtime, &copied, &second, &Ok(stored_second));
    copied_executor
        .correct(third.clone())
        .expect("repaired chain admits one item");
    assert_durable_state(&runtime, &copied, &tenant, thread, turn, 4, 5);
    assert!(!runtime.block_on(terminal_flag(&copied, &first)));
    assert!(!runtime.block_on(terminal_flag(&copied, &third)));
    runtime.block_on(remove_copy(&pool, &copied, &schema));
    runtime.block_on(pool.close());
}

/// The stored retry's own successor shape matters even though the ancestry
/// walk begins at its predecessor and cannot count children of that Item.
#[test]
fn exact_retry_rejects_a_branch_at_the_stored_item() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    let tenant =
        TenantId::new(format!("cand11-retry-branch-{}", Uuid::new_v4())).expect("fixture tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let root = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let command = correction_command(&tenant, thread, turn, ItemId::new(), root);
    let public_executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let original = public_executor
        .correct(command.clone())
        .expect("admit correction");
    let schema = format!("cand11_retry_branch_{}", Uuid::new_v4().simple());
    let copied = runtime.block_on(copy_items(&pool, &schema, &command));
    let copied_executor = SqlxPostgresExecutor::new(copied.clone(), runtime.handle().clone());
    let child = correction_command(&tenant, thread, turn, ItemId::new(), command.item_id());
    copied_executor
        .correct(child.clone())
        .expect("one valid successor");
    assert_existing_result(&runtime, &copied, &command, &Ok(original.clone()));
    assert_durable_state(&runtime, &copied, &tenant, thread, turn, 3, 4);

    let extra = ItemId::new();
    runtime.block_on(add_extra_successor(&copied, &command, &child, extra));
    assert_existing_result(
        &runtime,
        &copied,
        &command,
        &Err(CorrectionError::CorruptHistory),
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
    assert_eq!(
        copied_executor.correct(content_drift),
        Err(CorrectionError::IdentityConflict)
    );
    assert_durable_state(&runtime, &copied, &tenant, thread, turn, 4, 5);

    runtime.block_on(remove_extra_successor(&copied, &command, extra));
    assert_existing_result(&runtime, &copied, &command, &Ok(original));
    assert_durable_state(&runtime, &copied, &tenant, thread, turn, 3, 4);
    runtime.block_on(remove_copy(&pool, &copied, &schema));
    runtime.block_on(pool.close());
}

/// Adds one copied successor with a distinct sequence, then advances the
/// fixture counter so only the duplicate edge makes stored history invalid.
async fn add_extra_successor(
    pool: &PgPool,
    command: &CorrectionCommand,
    child: &CorrectionCommand,
    extra: ItemId,
) {
    sqlx::query(
        "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
         item_type, payload, is_terminal, corrects_item_id) \
         SELECT tenant_id, thread_id, turn_id, 4, $3, item_type, payload, \
                is_terminal, corrects_item_id FROM turn_items \
         WHERE tenant_id = $1 AND item_id = $2",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(child.item_id().as_uuid())
    .bind(extra.as_uuid())
    .execute(pool)
    .await
    .expect("add a second successor in the private copy");
    set_fixture_counter(pool, command, 5).await;
}

/// Removes only the extra copied child and restores the valid Turn counter.
async fn remove_extra_successor(pool: &PgPool, command: &CorrectionCommand, extra: ItemId) {
    sqlx::query("DELETE FROM turn_items WHERE tenant_id = $1 AND item_id = $2")
        .bind(command.trust().tenant_id.as_str())
        .bind(extra.as_uuid())
        .execute(pool)
        .await
        .expect("remove extra copied successor");
    set_fixture_counter(pool, command, 4).await;
}

/// Keeps the Turn counter consistent with the private copied Item sequences.
async fn set_fixture_counter(pool: &PgPool, command: &CorrectionCommand, counter: i64) {
    sqlx::query(
        "UPDATE public.turns SET next_sequence = $4 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .bind(counter)
    .execute(pool)
    .await
    .expect("keep the fixture Turn counter consistent");
}

/// Checks the write and read-only exact-match owners against the same result.
fn assert_existing_result(
    runtime: &Runtime,
    pool: &PgPool,
    command: &CorrectionCommand,
    expected: &Result<Item, CorrectionError>,
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
    .expect("copy only this fixture's Item rows");
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
