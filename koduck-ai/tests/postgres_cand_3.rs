// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! AC-3: migration 0009 adds the correction schema additively, idempotently,
//! and under the production startup serialization contract against one
//! disposable production `PostgreSQL`, while preserving every pre-migration
//! CAND-1/CAND-2 row, constraint, and raw replay result and rejecting every
//! invalid correction structure at the durable boundary (ADR-0003 CR-02
//! through CR-06 and CR-08).

use std::time::Duration;

use koduck_ai::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use koduck_ai::application::{HistoryError, TurnHistory};
use koduck_ai::domain::execution::{ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus};
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, TurnId, Usage,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPool, PgPoolOptions, PgQueryResult};
use uuid::Uuid;

const MIGRATIONS: [&str; 9] = [
    include_str!("../migrations/0001_cand_1_history.sql"),
    include_str!("../migrations/0002_cand_2_policy_execution.sql"),
    include_str!("../migrations/0003_cand_2_requester_ownership.sql"),
    include_str!("../migrations/0004_cand_2_tool_projections.sql"),
    include_str!("../migrations/0005_cand_2_execution_attempts.sql"),
    include_str!("../migrations/0006_cand_2_interrupt_barrier.sql"),
    include_str!("../migrations/0007_cand_2_tool_audit.sql"),
    include_str!("../migrations/0008_cand_2_interruption_approval_cancellation.sql"),
    include_str!("../migrations/0009_cand_3_correction_items.sql"),
];

/// `koduck_ai::runtime::STARTUP_MIGRATION_LOCK_KEY`, replicated here because
/// the runtime module is crate-private; the value is the stable production
/// serialization contract.
const STARTUP_MIGRATION_LOCK_KEY: i64 = 0x6B6F_6475_636B_3031;

const INSERT_ITEM_SQL: &str = "INSERT INTO turn_items (tenant_id, thread_id, turn_id, \
     sequence, item_id, item_type, payload, is_terminal, corrects_item_id) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)";

struct Harness {
    runtime: tokio::runtime::Runtime,
    pool: PgPool,
}

struct Fixture {
    tenant: TenantId,
    thread: ThreadId,
    turn: TurnId,
    other_turn: TurnId,
    input_item_id: ItemId,
    delta_item_id: ItemId,
    other_turn_item_id: ItemId,
    approval_id: Uuid,
    attempt_id: Uuid,
}

/// Identities of the anonymous seeded Items, captured so the expected replay
/// can name them.
struct SeededIds {
    usage: Uuid,
    approval_item: Uuid,
    tool_call: Uuid,
    tool_result: Uuid,
    terminal: Uuid,
}

/// One raw `turn_items` insertion under test.
struct RawRow {
    sequence: i64,
    item_id: Uuid,
    item_type: &'static str,
    payload: String,
    is_terminal: bool,
    corrects: Option<Uuid>,
}

/// AC-3: the migration, replay, constraint, and recovery behavior of the
/// correction schema against a disposable production `PostgreSQL`.
#[test]
fn correction_schema_migration() {
    let Some(harness) = harness() else {
        eprintln!("KODUCK_AI_TEST_DATABASE_URL is not set; skipping AC-3");
        return;
    };
    let runtime = harness.runtime;
    let pool = harness.pool;

    // A startup sequence stalled on the serialization lock under its
    // deadline exposes no partially usable schema (CR-06).
    runtime.block_on(stalled_startup_exposes_no_partial_schema(&pool));

    // Pre-migration baseline: every CAND-1/CAND-2 Item kind and one
    // terminal Turn exist before migration 0009 is applied.
    runtime.block_on(apply(&pool, &MIGRATIONS[..8]));
    let (fixture, expected) = runtime.block_on(seed_canonical_history(&pool));

    // Two concurrent migration runners serialize on the production
    // advisory lock and both finish (CR-06 concurrency row).
    runtime.block_on(run_concurrent_migration_runners(&pool));

    runtime.block_on(assert_correction_schema(&pool));
    runtime.block_on(assert_baseline_schema_intact(&pool));

    // The pre-migration rows replay to their exact domain meaning after the
    // upgrade (CR-06).
    let replay_first = replay(&pool, runtime.handle(), &fixture, fixture.turn);
    assert_eq!(replay_first, expected);

    // A second full application succeeds: the migration is idempotent and
    // leaves the replay result identical.
    runtime.block_on(apply(&pool, &MIGRATIONS));
    let replay_second = replay(&pool, runtime.handle(), &fixture, fixture.turn);
    assert_eq!(replay_second, replay_first);
    assert_eq!(replay_hash(&replay_second), replay_hash(&replay_first));

    append_valid_correction_and_replay_it(&runtime, &pool, &fixture, &replay_first);
    reject_invalid_correction_structures(&runtime, &pool, &fixture);
    failed_statement_group_rolls_back_atomically(&runtime, &pool, &fixture);
    malformed_external_payload_fails_closed_at_replay(&runtime, &pool, &fixture);
}

fn harness() -> Option<Harness> {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").ok()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("PostgreSQL test runtime");
    let pool = runtime
        .block_on(
            PgPoolOptions::new()
                .max_connections(8)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    Some(Harness { runtime, pool })
}

/// Applies one migration slice directly, once per migration.
async fn apply(pool: &PgPool, migrations: &[&'static str]) {
    for migration in migrations {
        sqlx::raw_sql(*migration)
            .execute(pool)
            .await
            .expect("apply production migration");
    }
}

/// Mirrors `koduck_ai::runtime::apply_startup_migrations`: the complete
/// sequence inside one transaction serialized by the production advisory
/// lock.
async fn run_startup_migrations(pool: PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(STARTUP_MIGRATION_LOCK_KEY)
        .execute(&mut *transaction)
        .await?;
    for migration in MIGRATIONS {
        sqlx::raw_sql(migration).execute(&mut *transaction).await?;
    }
    transaction.commit().await
}

async fn run_concurrent_migration_runners(pool: &PgPool) {
    let runner_a = tokio::spawn(run_startup_migrations(pool.clone()));
    let runner_b = tokio::spawn(run_startup_migrations(pool.clone()));
    runner_a
        .await
        .expect("runner A joins")
        .expect("runner A applies the sequence");
    runner_b
        .await
        .expect("runner B joins")
        .expect("runner B applies the sequence");
}

#[derive(Debug, PartialEq)]
struct SchemaState {
    turn_items: Option<String>,
    correction_column: bool,
}

async fn schema_state(pool: &PgPool) -> SchemaState {
    let turn_items: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.turn_items')::text")
            .fetch_one(pool)
            .await
            .expect("read regclass state");
    let correction_column: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'turn_items' \
         AND column_name = 'corrects_item_id')",
    )
    .fetch_one(pool)
    .await
    .expect("read column state");
    SchemaState {
        turn_items,
        correction_column,
    }
}

/// Holds the production serialization lock from a second session, runs the
/// startup sequence under a short deadline, and proves the stalled run
/// exposes no partial schema.
async fn stalled_startup_exposes_no_partial_schema(pool: &PgPool) {
    let mut blocker = pool.acquire().await.expect("blocker connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(STARTUP_MIGRATION_LOCK_KEY)
        .execute(&mut *blocker)
        .await
        .expect("take the session lock");
    let before = schema_state(pool).await;
    let stalled = tokio::time::timeout(
        Duration::from_millis(500),
        run_startup_migrations(pool.clone()),
    )
    .await;
    assert!(
        stalled.is_err(),
        "the startup sequence must wait on the lock and hit its deadline"
    );
    let after = schema_state(pool).await;
    assert_eq!(
        after, before,
        "a stalled startup under its deadline exposes no partial schema"
    );
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(STARTUP_MIGRATION_LOCK_KEY)
        .execute(&mut *blocker)
        .await
        .expect("release the session lock");
}

/// Seeds every pre-0009 Item kind plus terminal Turn state before the
/// correction migration runs, and returns the exact expected replay.
async fn seed_canonical_history(pool: &PgPool) -> (Fixture, Vec<Item>) {
    let fixture = Fixture {
        tenant: TenantId::new(format!("cand3-{}", Uuid::new_v4())).expect("valid tenant"),
        thread: ThreadId::new(),
        turn: TurnId::new(),
        other_turn: TurnId::new(),
        input_item_id: ItemId::new(),
        delta_item_id: ItemId::new(),
        other_turn_item_id: ItemId::new(),
        approval_id: Uuid::new_v4(),
        attempt_id: Uuid::new_v4(),
    };
    seed_thread_and_turns(pool, &fixture).await;

    let ids = SeededIds {
        usage: Uuid::new_v4(),
        approval_item: Uuid::new_v4(),
        tool_call: Uuid::new_v4(),
        tool_result: Uuid::new_v4(),
        terminal: Uuid::new_v4(),
    };
    for row in seeded_rows(&fixture, &ids) {
        insert_row(pool, &fixture, fixture.turn, row)
            .await
            .expect("seed item");
    }
    insert_row(
        pool,
        &fixture,
        fixture.other_turn,
        RawRow {
            sequence: 1,
            item_id: fixture.other_turn_item_id.as_uuid(),
            item_type: "user_message",
            payload: "{\"content\":\"other turn\"}".to_owned(),
            is_terminal: false,
            corrects: None,
        },
    )
    .await
    .expect("seed other turn item");
    let expected = expected_seeded_replay(&fixture, &ids);
    (fixture, expected)
}

/// The exhaustive pre-0009 seed table for the main Turn: one row of every
/// existing Item kind plus the terminal state.
fn seeded_rows(fixture: &Fixture, ids: &SeededIds) -> Vec<RawRow> {
    vec![
        RawRow {
            sequence: 1,
            item_id: fixture.input_item_id.as_uuid(),
            item_type: "user_message",
            payload: "{\"content\":\"original\"}".to_owned(),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 2,
            item_id: fixture.delta_item_id.as_uuid(),
            item_type: "agent_message_delta",
            payload: "{\"content\":\"delta\"}".to_owned(),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 3,
            item_id: ids.usage,
            item_type: "usage",
            payload: "{\"input_tokens\":3,\"output_tokens\":5,\"total_tokens\":8}".to_owned(),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 4,
            item_id: ids.approval_item,
            item_type: "approval_status",
            payload: format!(
                "{{\"approval_id\":\"{}\",\"attempt_id\":\"{}\",\"status\":\"requested\",\
                 \"decision\":null,\"version\":1}}",
                fixture.approval_id, fixture.attempt_id
            ),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 5,
            item_id: ids.tool_call,
            item_type: "tool_call",
            payload: "{\"descriptor_id\":\"\",\"descriptor_version\":\"\",\
             \"target\":\"\",\"attempt_id\":null,\"status\":null,\"version\":null}"
                .to_owned(),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 6,
            item_id: ids.tool_result,
            item_type: "tool_result",
            payload: "{\"attempt_id\":null,\"status\":\"failed\",\
             \"code\":\"descriptor_missing\",\"effect_state\":null,\"output_bytes\":0,\
             \"output_digest\":null,\"version\":null}"
                .to_owned(),
            is_terminal: false,
            corrects: None,
        },
        RawRow {
            sequence: 7,
            item_id: ids.terminal,
            item_type: "completed",
            payload: "{\"input_tokens\":7,\"output_tokens\":11,\"total_tokens\":18}".to_owned(),
            is_terminal: true,
            corrects: None,
        },
    ]
}

async fn seed_thread_and_turns(pool: &PgPool, fixture: &Fixture) {
    sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
        .bind(fixture.tenant.as_str())
        .bind("subject-a")
        .bind(fixture.thread.as_uuid())
        .execute(pool)
        .await
        .expect("seed thread");
    for turn_id in [fixture.turn, fixture.other_turn] {
        sqlx::query(
            "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES ($1, $2, $3, 'completed', 3)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(turn_id.as_uuid())
        .execute(pool)
        .await
        .expect("seed turn");
    }
}

/// The exact expected raw replay of the seeded main Turn.
fn expected_seeded_replay(fixture: &Fixture, ids: &SeededIds) -> Vec<Item> {
    vec![
        seeded_item(
            fixture.input_item_id.as_uuid(),
            1,
            ItemPayload::UserMessage {
                content: "original".to_owned(),
            },
        ),
        seeded_item(
            fixture.delta_item_id.as_uuid(),
            2,
            ItemPayload::AgentMessageDelta {
                content: "delta".to_owned(),
            },
        ),
        seeded_item(
            ids.usage,
            3,
            ItemPayload::Usage(Usage::new(3, 5).expect("valid usage")),
        ),
        seeded_item(
            ids.approval_item,
            4,
            ItemPayload::ApprovalStatus {
                approval_id: ApprovalId::from_uuid(fixture.approval_id),
                attempt_id: AttemptId::from_uuid(fixture.attempt_id),
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
        ),
        seeded_item(
            ids.tool_call,
            5,
            ItemPayload::ToolCall {
                descriptor_id: String::new(),
                descriptor_version: String::new(),
                target: String::new(),
                attempt_id: None,
                status: None,
                version: None,
            },
        ),
        seeded_item(
            ids.tool_result,
            6,
            ItemPayload::ToolResult {
                attempt_id: None,
                status: ExecutionStatus::Failed,
                code: Some("descriptor_missing".to_owned()),
                effect_state: None,
                output_bytes: 0,
                output_digest: None,
                version: None,
            },
        ),
        seeded_item(
            ids.terminal,
            7,
            ItemPayload::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(7, 11).expect("valid usage"),
            }),
        ),
    ]
}

fn seeded_item(item_id: Uuid, sequence: u64, payload: ItemPayload) -> Item {
    Item {
        item_id: ItemId::from_uuid(item_id),
        sequence,
        payload,
    }
}

fn correction_row(sequence: i64, content: &str, corrects: ItemId) -> RawRow {
    RawRow {
        sequence,
        item_id: Uuid::new_v4(),
        item_type: "correction",
        payload: serde_json::json!({ "content": content }).to_string(),
        is_terminal: false,
        corrects: Some(corrects.as_uuid()),
    }
}

async fn insert_row(
    pool: &PgPool,
    fixture: &Fixture,
    turn: TurnId,
    row: RawRow,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(INSERT_ITEM_SQL)
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(turn.as_uuid())
        .bind(row.sequence)
        .bind(row.item_id)
        .bind(row.item_type)
        .bind(row.payload)
        .bind(row.is_terminal)
        .bind(row.corrects)
        .execute(pool)
        .await
}

fn expect_rejection(result: &Result<PgQueryResult, sqlx::Error>, expected_object: &str) {
    match result {
        Err(sqlx::Error::Database(error)) => assert!(
            error.message().contains(expected_object),
            "expected rejection by {expected_object}, got: {}",
            error.message()
        ),
        other => panic!("expected rejection by {expected_object}, got: {other:?}"),
    }
}

/// A valid correction persists structurally and raw replay returns every
/// original and correction Item exactly once in sequence order (CR-01,
/// CR-04).
fn append_valid_correction_and_replay_it(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    fixture: &Fixture,
    replay_first: &[Item],
) {
    runtime
        .block_on(insert_row(
            pool,
            fixture,
            fixture.turn,
            correction_row(8, "fixed user text", fixture.input_item_id),
        ))
        .expect("valid correction inserts");
    let corrected = replay(pool, runtime.handle(), fixture, fixture.turn);
    assert_eq!(corrected.len(), 8);
    assert_eq!(corrected[..7], replay_first[..]);
    assert_eq!(corrected[7].sequence, 8);
    let ItemPayload::Correction(correction) = &corrected[7].payload else {
        panic!("the appended correction must replay as the typed payload");
    };
    assert_eq!(correction.content(), "fixed user text");
    assert_eq!(correction.corrects_item_id(), fixture.input_item_id);
}

/// Invalid structures are rejected by the durable constraints (CR-02,
/// CR-03): self-reference, one-predecessor-one-successor, same-Turn scope,
/// and the discriminator/relationship/terminal shape pairing.
fn reject_invalid_correction_structures(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    fixture: &Fixture,
) {
    let self_id = Uuid::new_v4();
    let shape = |item_type: &'static str, terminal: bool, corrects: Option<Uuid>| RawRow {
        sequence: 9,
        item_id: Uuid::new_v4(),
        item_type,
        payload: "{\"content\":\"shape\"}".to_owned(),
        is_terminal: terminal,
        corrects,
    };
    let cases: [(&str, RawRow); 7] = [
        (
            "turn_items_correction_not_self",
            RawRow {
                item_id: self_id,
                corrects: Some(self_id),
                ..correction_row(9, "self", ItemId::new())
            },
        ),
        (
            "turn_items_one_direct_correction",
            correction_row(9, "second successor", fixture.input_item_id),
        ),
        (
            "turn_items_correction_scope",
            correction_row(9, "cross turn", fixture.other_turn_item_id),
        ),
        (
            "turn_items_correction_scope",
            correction_row(9, "unknown target", ItemId::new()),
        ),
        (
            "turn_items_correction_shape",
            shape("user_message", false, Some(fixture.input_item_id.as_uuid())),
        ),
        (
            "turn_items_correction_shape",
            shape("correction", false, None),
        ),
        (
            "turn_items_correction_shape",
            shape("correction", true, Some(fixture.delta_item_id.as_uuid())),
        ),
    ];
    for (expected_object, row) in cases {
        let result = runtime.block_on(insert_row(pool, fixture, fixture.turn, row));
        expect_rejection(&result, expected_object);
    }
}

/// A failed statement group rolls back atomically (CR-08): the valid
/// correction written before the failing statement never persists.
fn failed_statement_group_rolls_back_atomically(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    fixture: &Fixture,
) {
    runtime.block_on(async {
        let mut transaction = pool.begin().await.expect("rollback transaction begins");
        sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             VALUES ($1, $2, $3, 9, $4, 'correction', '{\"content\":\"lost\"}', FALSE, $5)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(Uuid::new_v4())
        .bind(fixture.delta_item_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .expect("the valid correction inserts inside the transaction");
        let rolled_back_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             VALUES ($1, $2, $3, 10, $4, 'correction', '{\"content\":\"self\"}', FALSE, $4)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(rolled_back_id)
        .execute(&mut *transaction)
        .await
        .expect_err("the failing statement aborts the transaction");
        drop(transaction);
    });
    let after_rollback = replay(pool, runtime.handle(), fixture, fixture.turn);
    assert_eq!(
        after_rollback.len(),
        8,
        "the rolled-back correction never persists"
    );
}

/// Externally malformed correction payloads fail closed at replay and are
/// never guessed, dropped, or rewritten (CR-05).
fn malformed_external_payload_fails_closed_at_replay(
    runtime: &tokio::runtime::Runtime,
    pool: &PgPool,
    fixture: &Fixture,
) {
    runtime
        .block_on(insert_row(
            pool,
            fixture,
            fixture.other_turn,
            RawRow {
                sequence: 2,
                item_id: Uuid::new_v4(),
                item_type: "correction",
                payload: "{\"content\":\"\"}".to_owned(),
                is_terminal: false,
                corrects: Some(fixture.other_turn_item_id.as_uuid()),
            },
        ))
        .expect("the malformed payload passes the durable structure constraints");
    assert_eq!(
        replay_result(pool, runtime.handle(), &fixture.tenant, fixture.other_turn),
        Err(HistoryError::Unavailable),
        "a malformed externally inserted correction payload fails closed at replay"
    );
}

fn replay(
    pool: &PgPool,
    handle: &tokio::runtime::Handle,
    fixture: &Fixture,
    turn: TurnId,
) -> Vec<Item> {
    replay_result(pool, handle, &fixture.tenant, turn).expect("replay succeeds")
}

fn replay_result(
    pool: &PgPool,
    handle: &tokio::runtime::Handle,
    tenant: &TenantId,
    turn: TurnId,
) -> Result<Vec<Item>, HistoryError> {
    let executor = SqlxPostgresExecutor::new(pool.clone(), handle.clone());
    let history = PostgresTurnHistory::new(executor);
    TurnHistory::replay(&history, tenant, turn)
}

fn replay_hash(items: &[Item]) -> String {
    let digest = Sha256::digest(format!("{items:?}").as_bytes());
    digest
        .iter()
        .fold(String::new(), |hex, byte| format!("{hex}{byte:02x}"))
}

async fn assert_correction_schema(pool: &PgPool) {
    let item_type_def: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conrelid = 'turn_items'::regclass \
         AND conname = 'turn_items_item_type_check'",
    )
    .fetch_one(pool)
    .await
    .expect("item type constraint exists");
    assert!(
        item_type_def.contains("'correction'"),
        "the correction discriminator must be admitted: {item_type_def}"
    );
    for object in [
        "turn_items_correction_shape",
        "turn_items_correction_not_self",
        "turn_items_correction_scope",
    ] {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_constraint \
             WHERE conrelid = 'turn_items'::regclass AND conname = $1",
        )
        .bind(object)
        .fetch_one(pool)
        .await
        .expect("constraint count reads");
        assert_eq!(count, 1, "constraint {object} must exist exactly once");
    }
    let index_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_indexes WHERE indexname = 'turn_items_one_direct_correction'",
    )
    .fetch_one(pool)
    .await
    .expect("index count reads");
    assert_eq!(
        index_count, 1,
        "the successor index must exist exactly once"
    );
}

async fn assert_baseline_schema_intact(pool: &PgPool) {
    for index in [
        "turn_items_one_terminal_per_turn",
        "turn_items_thread_replay",
    ] {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_indexes WHERE indexname = $1")
            .bind(index)
            .fetch_one(pool)
            .await
            .expect("baseline index count reads");
        assert_eq!(count, 1, "baseline index {index} must remain");
    }
    let primary_key: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint \
         WHERE conrelid = 'turn_items'::regclass AND contype = 'p'",
    )
    .fetch_one(pool)
    .await
    .expect("primary key count reads");
    assert_eq!(primary_key, 1, "the turn_items primary key must remain");
}
