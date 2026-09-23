// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! CA-04/CA-05: an exact retry cannot hide a stale Turn-wide sequence counter.
//! The production correction port and read-only reconciler must agree without
//! allocating another sequence or changing durable rows.

use std::time::Duration;

use sqlx::PgPool;
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::adapters::history::postgres::SqlxPostgresExecutor;
use crate::application::{CorrectionCommand, CorrectionError, CorrectionStore};
use crate::domain::{Item, ItemId, TenantId, ThreadId, TurnId};

use super::commit_ack_loss::{
    assert_durable_state, connected_pool, correction_command, seed_completed_turn, seeded_input_id,
};
use super::payload_read_race::{connect_scoped_reader, wait_for_barrier};

/// Exercises stale, recovered, and exhausted-but-valid counters on one tiny
/// history, including an unrelated higher sequence in another Turn.
#[test]
fn exact_retry_validates_the_turn_wide_counter() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    let tenant =
        TenantId::new(format!("cand11-retry-counter-{}", Uuid::new_v4())).expect("fixture tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let root = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let command = correction_command(&tenant, thread, turn, ItemId::new(), root);
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let original = executor.correct(command.clone()).expect("admit correction");
    let unrelated = runtime.block_on(seed_unrelated_items(&pool, &command));

    for (sequence, counter, expected) in [
        (3, 3, Err(CorrectionError::CorruptHistory)),
        (4, 3, Err(CorrectionError::CorruptHistory)),
        (4, 5, Ok(original.clone())),
        (4, i64::MAX, Ok(original)),
    ] {
        runtime.block_on(set_sequence_state(
            &pool, &command, unrelated, sequence, counter,
        ));
        assert_retry_outcome(&runtime, &pool, &command, &expected, counter);
    }

    // CA-04 identity drift wins even when CA-05 corruption is also present.
    runtime.block_on(set_sequence_state(&pool, &command, unrelated, 4, 3));
    let mismatch = CorrectionCommand::new(
        command.trust().clone(),
        thread,
        turn,
        command.item_id(),
        root,
        "different replacement",
    )
    .expect("valid mismatched content");
    assert_retry_outcome(
        &runtime,
        &pool,
        &mismatch,
        &Err(CorrectionError::IdentityConflict),
        3,
    );
    runtime.block_on(pool.close());
}

/// CA-04/CA-05/CA-07: a lawful concurrent append cannot make reconciliation
/// compare an old counter with a newer maximum and invent corruption.
#[test]
fn reconciliation_counter_snapshot_survives_a_concurrent_append() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    let tenant = TenantId::new(format!("cand11-counter-snapshot-{}", Uuid::new_v4()))
        .expect("fixture tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    runtime.block_on(seed_completed_turn(&pool, &tenant, &thread, &turn));
    let root = runtime.block_on(seeded_input_id(&pool, &tenant, &thread, &turn));
    let command = correction_command(&tenant, thread, turn, ItemId::new(), root);
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let original = executor.correct(command.clone()).expect("admit correction");
    let unrelated = runtime.block_on(seed_unrelated_items(&pool, &command));
    runtime.block_on(set_sequence_state(&pool, &command, unrelated, 3, 4));
    let writer = correction_command(
        &tenant,
        thread,
        turn,
        ItemId::new(),
        ItemId::from_uuid(unrelated),
    );
    let schema = format!("cand11_counter_snapshot_{}", Uuid::new_v4().simple());
    let key = i64::from_ne_bytes(*Uuid::new_v4().as_bytes().first_chunk().expect("key bytes"));
    let reader = runtime.block_on(create_counter_projection(&pool, &schema, turn, key));

    let (observed, appended) = runtime.block_on(append_during_counter_read(
        &pool, &reader, command, writer, key,
    ));
    runtime.block_on(reader.close());
    runtime.block_on(async {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .expect("remove counter snapshot fixture");
    });
    assert_eq!(observed, Ok(Some(original)));
    assert_eq!(appended.sequence, 4);
    assert_durable_state(&runtime, &pool, &tenant, thread, turn, 4, 5);
    runtime.block_on(pool.close());
}

/// Pauses only the selected Turn counter's projection; the real writer uses
/// the production tables and commits through the normal correction transaction.
async fn create_counter_projection(pool: &PgPool, schema: &str, turn: TurnId, key: i64) -> PgPool {
    // All interpolated identifiers and values are generated by this fixture.
    // The CA-05 schema token selects a counter read rather than an unused
    // view projection during ownership lookup; no source text is inspected.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; \
         CREATE FUNCTION {schema}.paused_counter(value BIGINT) RETURNS BIGINT \
         LANGUAGE plpgsql VOLATILE AS $body$ BEGIN \
         IF position('next_sequence' IN current_query()) > 0 THEN \
         PERFORM pg_advisory_xact_lock({key}); END IF; RETURN value; END $body$; \
         CREATE VIEW {schema}.turns AS SELECT tenant_id, thread_id, turn_id, status, \
         CASE WHEN turn_id = '{}'::uuid THEN {schema}.paused_counter(next_sequence) \
              ELSE next_sequence END AS next_sequence FROM public.turns",
        turn.as_uuid()
    )))
    .execute(pool)
    .await
    .expect("create counter projection barrier");
    connect_scoped_reader(schema).await
}

/// Uses three connections: one reader, one independent production writer,
/// and one lock controller that observes the actual server-side read barrier.
async fn append_during_counter_read(
    pool: &PgPool,
    reader: &PgPool,
    command: CorrectionCommand,
    writer: CorrectionCommand,
    key: i64,
) -> (Result<Option<Item>, CorrectionError>, Item) {
    let mut controller = pool.acquire().await.expect("lock controller");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(reader)
        .await
        .expect("reader identity");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *controller)
        .await
        .expect("hold counter projection");
    let writer_step = async {
        wait_for_barrier(&mut controller, pid).await;
        let appended = super::correct_async(pool, writer)
            .await
            .map_err(|error| match error {
                super::WriteFailure::Resolved(error) => error,
                super::WriteFailure::Ambiguous => CorrectionError::Unavailable,
            })
            .expect("independent production append");
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(key)
            .execute(&mut *controller)
            .await
            .expect("release old counter projection");
        appended
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(super::reconcile_async(reader, command), writer_step)
    })
    .await
    .expect("bounded three-connection snapshot race")
}

/// Adds an independent root in the owned Turn and a higher root in another
/// Turn of the same Thread, so only the owned Turn may affect its maximum.
async fn seed_unrelated_items(pool: &PgPool, command: &CorrectionCommand) -> Uuid {
    let unrelated = insert_root(pool, command, command.turn_id(), 3).await;
    let other_turn = TurnId::new();
    sqlx::query(
        "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
         VALUES ($1, $2, $3, 'completed', 101)",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(other_turn.as_uuid())
    .execute(pool)
    .await
    .expect("seed another Turn in the same Thread");
    insert_root(pool, command, other_turn, 100).await;
    unrelated
}

/// Creates one lawful message root without advancing the fixture counter.
async fn insert_root(
    pool: &PgPool,
    command: &CorrectionCommand,
    turn: TurnId,
    sequence: i64,
) -> Uuid {
    let identity = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
         item_type, payload, is_terminal, corrects_item_id) \
         VALUES ($1, $2, $3, $4, $5, 'user_message', \
                 '{\"content\":\"independent root\"}', FALSE, NULL)",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(turn.as_uuid())
    .bind(sequence)
    .bind(identity)
    .execute(pool)
    .await
    .expect("seed independent message root");
    identity
}

/// Models a stale restore and its repair before either reader is invoked.
async fn set_sequence_state(
    pool: &PgPool,
    command: &CorrectionCommand,
    unrelated: Uuid,
    sequence: i64,
    counter: i64,
) {
    sqlx::query("UPDATE turn_items SET sequence = $3 WHERE tenant_id = $1 AND item_id = $2")
        .bind(command.trust().tenant_id.as_str())
        .bind(unrelated)
        .bind(sequence)
        .execute(pool)
        .await
        .expect("set the independent Item sequence");
    sqlx::query(
        "UPDATE turns SET next_sequence = $4 \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(command.trust().tenant_id.as_str())
    .bind(command.thread_id().as_uuid())
    .bind(command.turn_id().as_uuid())
    .bind(counter)
    .execute(pool)
    .await
    .expect("set the Turn counter");
}

/// Verifies both production entry points and durable row/counter preservation.
fn assert_retry_outcome(
    runtime: &Runtime,
    pool: &PgPool,
    command: &CorrectionCommand,
    expected: &Result<Item, CorrectionError>,
    counter: i64,
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
        3,
        counter,
    );
}
