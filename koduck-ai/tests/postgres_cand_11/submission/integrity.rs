// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 durability-integrity cases: migration idempotence, commit
//! reconciliation through the deferred-trigger fixture, stored-payload
//! bounding, bounded history reads, and the production run assembly.

use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::application::{HistoryError, IdentifiedAcceptance, TurnCommand, TurnHistory};
use koduck_ai::domain::{SubmissionId, ThreadId, TurnId};

use super::super::harness::{Harness, MIGRATIONS};
use super::{connected_history, count_rows, identified_command, trust};

fn migration_fixture() -> (
    Harness,
    sqlx::PgPool,
    PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor>,
    String,
    String,
    String,
) {
    let harness = Harness::connect(4);
    let database_url =
        std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
    let schema = format!("cand18_mig_{}", Uuid::new_v4().simple());
    harness.runtime.block_on(async {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&harness.pool)
            .await
            .expect("create the migration fixture schema");
    });
    // The fixture pool keeps the production PgPoolOptions::new() defaults and
    // adds only the fixture search_path, so the unqualified startup DDL
    // resolves inside the isolated schema.
    let options = sqlx::postgres::PgConnectOptions::from_str(&database_url)
        .expect("valid test database URL")
        .options([("search_path", schema.clone())]);
    let pool = harness
        .runtime
        .block_on(sqlx::postgres::PgPoolOptions::new().connect_with(options))
        .expect("connect the migration fixture pool");
    // Populate migrations 0001..0009 first: the fixture starts pre-0010.
    harness.runtime.block_on(async {
        for migration in &MIGRATIONS[..9] {
            sqlx::raw_sql(sqlx::AssertSqlSafe((*migration).to_owned()))
                .execute(&pool)
                .await
                .expect("apply the pre-0010 migration sequence");
        }
    });
    let executor = SqlxPostgresExecutor::new(pool.clone(), harness.handle());
    let history = PostgresTurnHistory::new(executor);
    let tenant = format!("cand18-migration-{}", Uuid::new_v4());
    (harness, pool, history, tenant, schema, database_url)
}

/// AC-8 (ADR-0018): the exact production startup sequence applies twice on an
/// isolated populated fixture with no duplicate relation, changed legacy row,
/// or backfill, and one binding survives reapplication.
/// Seeds one legacy unidentified Turn on the pre-0010 fixture and returns
fn seed_legacy_turn(
    harness: &Harness,
    pool: &sqlx::PgPool,
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    tenant: &str,
) -> (TurnId, i64) {
    // The production port drives its own runtime; only the raw count rides
    // the harness handle, so the two never nest.
    let command = TurnCommand::new(
        trust(tenant, "subject-a"),
        None,
        "legacy before migration".to_owned(),
    )
    .expect("valid legacy command");
    let seeded = TurnHistory::accept_initial(history, &command)
        .expect("the pre-0010 legacy acceptance creates");
    let rows: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM turn_items WHERE turn_id = $1")
            .bind(seeded.turn_id.as_uuid())
            .fetch_one(pool)
            .await
            .expect("count the seeded legacy items")
    });
    (seeded.turn_id, rows)
}

/// Counts the seeded legacy Turn's durable items.
async fn legacy_item_count(pool: &sqlx::PgPool, turn_id: TurnId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM turn_items WHERE turn_id = $1")
        .bind(turn_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("count the legacy items")
}

/// Drops the isolated fixture schema after closing its pool.
async fn drop_migration_fixture(database_url: &str, schema: &str, pool: sqlx::PgPool) {
    pool.close().await;
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .expect("reconnect for fixture teardown");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .expect("drop the migration fixture schema");
    admin.close().await;
}

pub(crate) fn cand_18_migration_and_integrity() {
    let _database_guard = super::serialize_database_tests();
    let (harness, pool, mut history, tenant, schema, database_url) = migration_fixture();
    let submission = Uuid::new_v4();

    // Seed legacy state while the fixture is still pre-0010: unidentified
    // acceptance needs only 0001..0009 and must stay untouched by 0010.
    let (seeded_turn, legacy_rows) = seed_legacy_turn(&harness, &pool, &mut history, &tenant);
    assert_eq!(
        legacy_rows, 1,
        "the legacy Turn carries its sequence-1 input"
    );

    // The exact startup sequence, applied twice through the production entry.
    for _ in 0..2 {
        harness
            .runtime
            .block_on(koduck_ai::runtime::apply_startup_migrations(
                &pool,
                Duration::from_secs(10),
            ))
            .expect("the startup sequence is idempotent");
    }
    let legacy_after = harness
        .runtime
        .block_on(legacy_item_count(&pool, seeded_turn));
    assert_eq!(
        legacy_rows, legacy_after,
        "reapplication changes no legacy rows and backfills nothing"
    );

    // With 0010 present, the identified acceptance works on the migrated
    // fixture and its binding survives a further full reapplication.
    let command = identified_command(&tenant, "subject-a", submission, None, "after migration");
    let IdentifiedAcceptance::Created(_) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the identified acceptance creates on the migrated fixture")
    else {
        panic!("the identified acceptance must create");
    };
    harness
        .runtime
        .block_on(koduck_ai::runtime::apply_startup_migrations(
            &pool,
            Duration::from_secs(10),
        ))
        .expect("a third full reapplication stays idempotent");
    let binding_count: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT count(*) FROM chat_submissions WHERE tenant_id = $1 AND submission_id = $2",
        )
        .bind(&tenant)
        .bind(submission)
        .fetch_one(&pool)
        .await
        .expect("count bindings after reapplication")
    });
    assert_eq!(
        binding_count, 1,
        "reapplication creates no duplicate binding"
    );

    harness
        .runtime
        .block_on(drop_migration_fixture(&database_url, &schema, pool));
}

/// The isolated AC-5 fixture: a private schema holding the complete canonical
/// migrations, one production-shaped pool bound to it through `search_path`,
pub(super) struct SubmissionFixture {
    database_url: String,
    schema: String,
    pub(super) tenant: String,
    pub(super) pool: sqlx::PgPool,
    pub(super) harness: Harness,
}

impl SubmissionFixture {
    pub(super) fn create(label: &str, trigger_body: &str) -> SubmissionFixture {
        let harness = Harness::connect(4);
        let database_url =
            std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
        let schema = format!("cand18_submission_{}", Uuid::new_v4().simple());
        let tenant = format!("cand18-reconcile-{label}-{}", Uuid::new_v4());
        harness.runtime.block_on(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&harness.pool)
                .await
                .expect("create the reconciliation fixture schema");
        });
        let options = sqlx::postgres::PgConnectOptions::from_str(&database_url)
            .expect("valid test database URL")
            .options([("search_path", schema.clone())]);
        let pool = harness
            .runtime
            .block_on(sqlx::postgres::PgPoolOptions::new().connect_with(options))
            .expect("connect the fixture pool");
        // The fixture carries the complete canonical schema, 0010 included,
        // through the exact production startup sequence; the trigger installs
        // afterwards, once its relation exists.
        harness
            .runtime
            .block_on(koduck_ai::runtime::apply_startup_migrations(
                &pool,
                Duration::from_secs(20),
            ))
            .expect("apply the canonical migrations into the fixture schema");
        harness.runtime.block_on(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE FUNCTION {schema}.cand18_commit_trigger() RETURNS trigger AS \
                 $body$ BEGIN IF NEW.tenant_id = $tenant${tenant}$tenant$ THEN \
                 {trigger_body} END IF; RETURN NEW; END; $body$ LANGUAGE plpgsql; \
                 CREATE CONSTRAINT TRIGGER cand18_commit_deferred \
                 AFTER INSERT ON {schema}.chat_submissions \
                 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
                 EXECUTE FUNCTION {schema}.cand18_commit_trigger()"
            )))
            .execute(&pool)
            .await
            .expect("install the deferred commit trigger");
        });
        SubmissionFixture {
            database_url,
            schema,
            tenant,
            pool,
            harness,
        }
    }

    pub(super) fn history(
        &self,
    ) -> PostgresTurnHistory<koduck_ai::adapters::history::postgres::SqlxPostgresExecutor> {
        PostgresTurnHistory::new(
            koduck_ai::adapters::history::postgres::SqlxPostgresExecutor::new(
                self.pool.clone(),
                self.harness.handle(),
            ),
        )
    }

    pub(super) fn teardown(self) {
        let SubmissionFixture {
            database_url,
            schema,
            tenant: _tenant,
            pool,
            harness,
        } = self;
        harness.runtime.block_on(async move {
            pool.close().await;
            let admin = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("reconnect for fixture teardown");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE"
            )))
            .execute(&admin)
            .await
            .expect("drop the reconciliation fixture schema");
            admin.close().await;
        });
    }
}

/// AC-5/SI-06 (ADR-0018): a delayed deferred-trigger commit whose write
/// acknowledgement times out is reconciled by the same invocation's read-only
/// proof, which observes the live creator and still returns the created
/// owner; the deferred-exception variant commits nothing and stays
pub(crate) fn cand_18_commit_reconciliation() {
    let _database_guard = super::serialize_database_tests();
    // Variant one: the commit survives the dropped acknowledgement.
    {
        let fixture = SubmissionFixture::create("delay", "PERFORM pg_sleep(2.5);");
        let mut history = fixture.history();
        let submission = Uuid::new_v4();
        let command = identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            None,
            "reconciled input",
        );
        let started = std::time::Instant::now();
        let outcome = TurnHistory::accept_initial_with_submission(
            &mut history,
            &command,
            Duration::from_secs(2),
        )
        .expect("the delayed commit reconciles to the created owner");
        let elapsed = started.elapsed();
        let IdentifiedAcceptance::Created(accepted) = outcome else {
            panic!("the same invocation must prove its live creator");
        };
        assert_eq!(accepted.generation.get(), 1);
        assert!(
            elapsed >= Duration::from_secs(2),
            "the write attempt consumed its full budget before the proof"
        );
        let rows = fixture.harness.runtime.block_on(count_rows(
            &fixture.pool,
            &fixture.tenant,
            "subject-a",
            submission,
        ));
        assert_eq!(rows, (1, 1, 1, 1), "exactly one durable acceptance exists");
        fixture.teardown();
    }

    // Variant two: the deferred trigger aborts the commit, so no binding and
    // no canonical state exists and the outcome stays unavailable.
    {
        let fixture =
            SubmissionFixture::create("abort", "RAISE EXCEPTION 'cand18 controlled commit fault';");
        let mut history = fixture.history();
        let submission = Uuid::new_v4();
        let command = identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            None,
            "aborted input",
        );
        assert_eq!(
            TurnHistory::accept_initial_with_submission(
                &mut history,
                &command,
                Duration::from_secs(2),
            )
            .expect_err("the aborted commit stays unavailable"),
            HistoryError::Unavailable
        );
        let rows = fixture.harness.runtime.block_on(count_rows(
            &fixture.pool,
            &fixture.tenant,
            "subject-a",
            submission,
        ));
        assert_eq!(
            rows,
            (0, 0, 0, 0),
            "the aborted commit leaves zero partial rows"
        );
        fixture.teardown();
    }
}

/// The production `run` assembly — pool construction, migration application,
/// adapter wiring, and the explicit system acceptance clock — executes up to
pub(crate) fn cand_18_runtime_assembly() {
    let _database_guard = super::serialize_database_tests();
    let harness = Harness::connect(4);
    let database_url =
        std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("isolated test database URL");
    // A bound listener makes the assembly's bind step fail immediately, so
    // every preceding assembly line runs inside the test.
    let probe_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("probe port");
    let bind_addr = probe_listener.local_addr().expect("probe address");
    let environment = std::collections::BTreeMap::from([
        ("KODUCK_AI_BIND_ADDR".to_owned(), bind_addr.to_string()),
        ("KODUCK_AI_DATABASE_URL".to_owned(), database_url),
        (
            "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
            "https://provider.example/v1".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_MODEL".to_owned(),
            "provider-model".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_API_KEY".to_owned(),
            "not-a-real-secret".to_owned(),
        ),
    ]);
    let config = koduck_ai::runtime::RuntimeConfig::from_environment(&environment)
        .expect("the assembly environment validates");
    let outcome = harness
        .runtime
        .block_on(async move { koduck_ai::runtime::run(config).await });
    assert!(
        matches!(outcome, Err(koduck_ai::runtime::RuntimeError::Bind(_))),
        "the assembly reaches the listener bind and fails on the occupied port"
    );
    drop(probe_listener);
}

/// A history double that panics on every identified-port call, proving the
/// SI-01d direct-command guard fires before any history I/O.
#[test]
fn cand_18_oversized_stored_input_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Corrupt the stored sequence-1 payload beyond the input bound.
    harness.runtime.block_on(async {
        let oversized = format!(r#"{{"content":"{}"}}"#, "a".repeat(70_000));
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(oversized)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored input");
    });

    // The exact same request now fails unavailable, never a conflict.
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("the oversized stored input is corrupt structure"),
        HistoryError::Unavailable
    );
    // The transaction recheck keeps the same classification.
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the acceptance recheck stays unavailable"),
        HistoryError::Unavailable
    );
}

/// SI-01d (review round 2, finding 2): the direct observation port rejects an
/// invalid identified command before any database query — proven by an
/// exclusive table lock that would otherwise stall the lookup until its
#[test]
fn cand_18_observation_entry_guard_precedes_query() {
    let _database_guard = super::serialize_database_tests();
    let (harness, history, tenant) = connected_history();
    let submission = Uuid::new_v4();

    // Hold an exclusive lock on the binding relation for the whole case.
    let lock_pool = harness.pool.clone();
    let mut lock = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("lock transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("LOCK TABLE chat_submissions IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .expect("hold the binding relation exclusively");
    });

    // Invalid commands return the typed unavailability immediately, without
    // waiting out the lookup deadline the locked query would consume.
    for input in [String::new(), "a".repeat(65_537)] {
        let command = TurnCommand {
            trust: trust(&tenant, "subject-a"),
            thread_id: None,
            input,
            submission_id: Some(SubmissionId::from_uuid(submission).expect("non-nil")),
        };
        let started = std::time::Instant::now();
        assert_eq!(
            history
                .submission_observation(&command, Duration::from_secs(2))
                .expect_err("the guard rejects the invalid command"),
            HistoryError::Unavailable
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the guard rejected in {:?} without the locked database query",
            started.elapsed()
        );
    }
    harness.runtime.block_on(async move {
        drop(lock);
    });
}

/// SI-07c (review round 3, finding 1): the bounded prior-history read honors
/// its caller-owned deadline — proven under an exclusive lock on the tables
/// it reads, where the fixed two-second budget and a 100 ms clamped budget
#[test]
fn cand_18_bounded_history_read_honors_deadline() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, tenant) = connected_history();
    let thread = ThreadId::new();
    harness.runtime.block_on(async {
        sqlx::query("INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)")
            .bind(&tenant)
            .bind("subject-a")
            .bind(thread.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("seed thread");
    });
    let executor = SqlxPostgresExecutor::new(harness.pool.clone(), harness.handle());

    let lock_pool = harness.pool.clone();
    let mut lock = harness
        .runtime
        .block_on(lock_pool.begin())
        .expect("lock transaction starts");
    harness.runtime.block_on(async {
        sqlx::query("LOCK TABLE turn_items IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .expect("hold the item relation exclusively");
    });

    // The clamped 100 ms budget rejects after ~100 ms, far inside the fixed
    // two-second budget an unclamped read would consume.
    let started = std::time::Instant::now();
    assert_eq!(
        executor
            .prior_thread_turns_bounded(
                &trust(&tenant, "subject-a"),
                thread,
                Duration::from_millis(100),
            )
            .expect_err("the clamped read times out"),
        HistoryError::Unavailable
    );
    let clamped_elapsed = started.elapsed();
    assert!(
        clamped_elapsed < Duration::from_millis(1_500),
        "the clamped read rejected in {clamped_elapsed:?}"
    );
    harness.runtime.block_on(async move {
        drop(lock);
    });
}

/// SI-09 (review round 3, finding 2): a stored sequence-1 payload that is
/// noncanonical (extra JSON members) or beyond the transport envelope fails
#[test]
fn cand_18_noncanonical_and_oversized_payloads_fail_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Noncanonical structure: an extra member beside `content`.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(r#"{"content":"bounded input","junk":"unexpected"}"#)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored structure");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("noncanonical stored structure fails unavailable"),
        HistoryError::Unavailable
    );

    // Beyond the transport envelope: the payload is never compared.
    harness.runtime.block_on(async {
        let oversized = format!(
            r#"{{"content":"bounded input","junk":"{}"}}"#,
            "b".repeat(500_000)
        );
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(oversized)
        .execute(&harness.pool)
        .await
        .expect("push the stored payload past the envelope");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("the oversized envelope fails unavailable"),
        HistoryError::Unavailable
    );
}

/// SI-01d (review round 4, finding 1): the legacy runner entry points reject
/// an identified command with the typed invalid-command result instead of
#[test]
fn cand_18_duplicate_member_payload_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Duplicate members collapse to the last value in plain serde_json; the
    // lookup must reject the inconsistent structure outright.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_items SET payload = $4 WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .bind(r#"{"content":"other","content":"bounded input"}"#)
        .execute(&harness.pool)
        .await
        .expect("corrupt the stored structure with duplicate members");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("duplicate members are inconsistent stored structure"),
        HistoryError::Unavailable
    );
}

/// SI-08c (review round 9, finding 2): a sequence-1 `user_message` row
/// marked terminal is inconsistent canonical structure — it occupies the
/// Turn's one-terminal slot — so every binding read fails unavailable
/// instead of returning a receipt.
#[test]
fn cand_18_terminal_marked_input_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "bounded input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the bounded acceptance creates")
    else {
        panic!("the bounded acceptance must create");
    };

    // Corrupt the stored sequence-1 row with the terminal flag.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_items SET is_terminal = TRUE WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(&tenant)
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("mark the stored input terminal");
    });

    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("a terminal-marked input row is inconsistent structure"),
        HistoryError::Unavailable
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the transaction recheck stays unavailable"),
        HistoryError::Unavailable
    );
}

/// SI-06a/SI-06e (review round 10): when the deferred commit corrupts the
/// sequence-1 row's item identity while keeping its content, the
/// reconciliation cannot prove this invocation's proposed input identity and
/// stays unavailable instead of granting `Created` or a receipt. The
/// corruption runs inside the writer's own COMMIT transaction, so the proof
/// deterministically observes the mismatched committed identity.
#[test]
fn cand_18_reconciliation_verifies_input_identity() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create(
        "identity",
        "PERFORM pg_sleep(2.5); \
         UPDATE turn_items i SET item_id = gen_random_uuid() \
         FROM chat_submissions s WHERE s.tenant_id = NEW.tenant_id \
         AND s.submission_id = NEW.submission_id \
         AND i.tenant_id = s.tenant_id AND i.thread_id = s.thread_id \
         AND i.turn_id = s.turn_id AND i.sequence = 1;",
    );
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "identity-corrupted input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the unproven input identity stays unavailable"),
        HistoryError::Unavailable
    );
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    assert_eq!(
        rows,
        (1, 1, 1, 1),
        "the delayed commit still produced exactly one durable acceptance"
    );
    fixture.teardown();
}

/// SI-06a/SI-06e (review round 11): when the deferred commit repoints this
/// invocation's binding at a twin Turn it seeds on the same Thread with
/// matching input, the private creator proves the row belongs to this
/// invocation, so the mismatched retained Turn identity is unprovable state —
/// the reconciliation stays unavailable instead of publishing the rewritten
/// identity as an observation receipt. The trigger performs the whole
/// rewrite inside the writer's own COMMIT transaction, so the proof
/// deterministically observes the retargeted committed binding.
#[test]
fn cand_18_reconciliation_rejects_rewritten_binding_target() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create(
        "retarget",
        "PERFORM pg_sleep(2.5); \
         INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
             VALUES (NEW.tenant_id, NEW.thread_id, gen_random_uuid(), 'started', 2); \
         INSERT INTO turn_leases (tenant_id, thread_id, turn_id, generation, \
             renewed_at, expires_at) SELECT tenant_id, thread_id, turn_id, 1, \
             CURRENT_TIMESTAMP, CURRENT_TIMESTAMP + INTERVAL '1 hour' \
             FROM turns WHERE tenant_id = NEW.tenant_id \
             AND thread_id = NEW.thread_id AND turn_id <> NEW.turn_id; \
         INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, \
             item_id, item_type, payload, is_terminal) SELECT i.tenant_id, \
             i.thread_id, t.turn_id, 1, gen_random_uuid(), 'user_message', \
             i.payload, FALSE FROM turn_items i JOIN turns t \
             ON t.tenant_id = i.tenant_id AND t.thread_id = i.thread_id \
             AND t.turn_id <> i.turn_id WHERE i.tenant_id = NEW.tenant_id \
             AND i.thread_id = NEW.thread_id AND i.turn_id = NEW.turn_id \
             AND i.sequence = 1; \
         UPDATE chat_submissions SET turn_id = ( \
             SELECT t.turn_id FROM turns t WHERE t.tenant_id = NEW.tenant_id \
             AND t.thread_id = NEW.thread_id AND t.turn_id <> NEW.turn_id \
             LIMIT 1) WHERE submission_id = NEW.submission_id;",
    );
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "retargeted input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the rewritten binding target stays unavailable"),
        HistoryError::Unavailable
    );
    let bindings: i64 = fixture.harness.runtime.block_on(async {
        sqlx::query_scalar("SELECT count(*) FROM chat_submissions WHERE tenant_id = $1")
            .bind(&fixture.tenant)
            .fetch_one(&fixture.pool)
            .await
            .expect("count the committed binding")
    });
    assert_eq!(bindings, 1, "the delayed commit still produced the binding");
    fixture.teardown();
}

/// SI-06a/SI-06e (review round 13): when the deferred commit rewrites this
/// invocation's sequence-1 input content while keeping its item identity,
/// Thread, Turn, and creator, the matching private creator proves the row
/// belongs to this invocation, so the mismatched committed input is corrupt,
/// unprovable evidence — the reconciliation stays unavailable instead of
/// classifying it as the client's drift conflict, which SI-02d reserves for
/// a genuinely different creator. The trigger rewrites the payload inside
/// the writer's own COMMIT transaction, so the proof deterministically
/// observes the changed committed content.
#[test]
fn cand_18_reconciliation_rejects_rewritten_input_content() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create(
        "content-rewrite",
        "PERFORM pg_sleep(2.5); \
         UPDATE turn_items i SET payload = '{\"content\":\"rewritten\"}' \
         FROM chat_submissions s WHERE s.tenant_id = NEW.tenant_id \
         AND s.submission_id = NEW.submission_id \
         AND i.tenant_id = s.tenant_id AND i.thread_id = s.thread_id \
         AND i.turn_id = s.turn_id AND i.sequence = 1;",
    );
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "content-corrupted input",
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the rewritten same-creator input stays unavailable"),
        HistoryError::Unavailable
    );
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    assert_eq!(
        rows,
        (1, 1, 1, 1),
        "the delayed commit still produced exactly one durable acceptance"
    );
    fixture.teardown();
}

/// SI-06c (review round 15): lease expiry is evaluated with the statement
/// clock after the submission lock is acquired, not with the transaction's
/// frozen `CURRENT_TIMESTAMP`. The deferred COMMIT rewrites this invocation's
/// lease to expire 150 ms later and then holds the writer's transaction — and
/// with it the submission advisory lock — for another second, so the
/// reconciliation transaction begins before the expiry but reads its proof
/// only after the lease is truly dead; a stale timestamp would grant the
/// same creator `Created` for an already expired lease.
#[test]
fn cand_18_lease_expiry_evaluated_after_lock_wait() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create(
        "fused-lease",
        "PERFORM pg_sleep(2.5); \
         UPDATE turn_leases SET renewed_at = clock_timestamp(), \
         expires_at = clock_timestamp() + INTERVAL '150 milliseconds' \
         WHERE tenant_id = NEW.tenant_id AND thread_id = NEW.thread_id \
         AND turn_id = NEW.turn_id; \
         PERFORM pg_sleep(1.0);",
    );
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "fused lease input",
    );
    let outcome =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the expired-lease proof resolves");
    let IdentifiedAcceptance::Existing(receipt) = &outcome else {
        panic!("an expired lease never grants Created, got {outcome:?}")
    };
    assert_eq!(
        receipt.submission_id.as_uuid(),
        submission,
        "the observation publishes only the client's identity"
    );
    let rows = fixture.harness.runtime.block_on(count_rows(
        &fixture.pool,
        &fixture.tenant,
        "subject-a",
        submission,
    ));
    assert_eq!(
        rows,
        (1, 1, 1, 1),
        "the delayed commit still produced exactly one durable acceptance"
    );
    fixture.teardown();
}

/// SI-08c (review round 15): a stored binding whose non-null original
/// selector differs from its accepted Thread is inconsistent structure no
/// valid writer can produce. Inside the isolated fixture schema the equality
/// CHECK is dropped to seed the corrupt row, and both a selectorless retry
/// and a retry carrying the corrupt selector must fail unavailable instead
/// of yielding a conflict or a valid-looking receipt for a different Thread.
#[test]
fn cand_18_inconsistent_stored_selector_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let fixture = SubmissionFixture::create("corrupt-selector", "PERFORM 1;");
    let mut history = fixture.history();
    let submission = Uuid::new_v4();
    let command = identified_command(
        &fixture.tenant,
        "subject-a",
        submission,
        None,
        "selector input",
    );
    let IdentifiedAcceptance::Created(_accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the selector fixture acceptance creates")
    else {
        panic!("the selector key must create");
    };
    let corrupt_selector = ThreadId::new();
    fixture.harness.runtime.block_on(async {
        sqlx::raw_sql(
            "ALTER TABLE chat_submissions \
             DROP CONSTRAINT chat_submissions_original_selector_check",
        )
        .execute(&fixture.pool)
        .await
        .expect("drop the fixture's equality check for the seed");
        sqlx::query(
            "UPDATE chat_submissions SET original_thread_id = $3 \
             WHERE tenant_id = $1 AND submission_id = $2",
        )
        .bind(&fixture.tenant)
        .bind(submission)
        .bind(corrupt_selector.as_uuid())
        .execute(&fixture.pool)
        .await
        .expect("seed the inconsistent stored selector");
    });
    for drifted in [
        command.clone(),
        identified_command(
            &fixture.tenant,
            "subject-a",
            submission,
            Some(corrupt_selector),
            "selector input",
        ),
    ] {
        assert_eq!(
            history
                .submission_observation(&drifted, Duration::from_secs(2))
                .expect_err("an inconsistent stored selector is corrupt structure"),
            HistoryError::Unavailable
        );
        assert_eq!(
            TurnHistory::accept_initial_with_submission(
                &mut history,
                &drifted,
                Duration::from_secs(2)
            )
            .expect_err("the transaction recheck stays unavailable"),
            HistoryError::Unavailable
        );
    }
    fixture.teardown();
}

/// SI-08c (review round 16): a Turn whose `next_sequence` is still 1 while
/// its sequence-1 user item exists is inconsistent canonical data — the
/// next append would collide at sequence 1 — so every binding lookup fails
/// unavailable instead of issuing a receipt for the corrupt state.
#[test]
fn cand_18_initial_sequence_counter_fails_unavailable() {
    let _database_guard = super::serialize_database_tests();
    let (harness, mut history, tenant) = connected_history();
    let submission = Uuid::new_v4();
    let command = identified_command(&tenant, "subject-a", submission, None, "counter input");
    let IdentifiedAcceptance::Created(accepted) =
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect("the counter fixture acceptance creates")
    else {
        panic!("the counter key must create");
    };
    harness.runtime.block_on(async {
        sqlx::query("UPDATE turns SET next_sequence = 1 WHERE tenant_id = $1 AND turn_id = $2")
            .bind(&tenant)
            .bind(accepted.turn_id.as_uuid())
            .execute(&harness.pool)
            .await
            .expect("rewind the turn's sequence counter");
    });
    assert_eq!(
        history
            .submission_observation(&command, Duration::from_secs(2))
            .expect_err("a rewound sequence counter is inconsistent structure"),
        HistoryError::Unavailable
    );
    assert_eq!(
        TurnHistory::accept_initial_with_submission(&mut history, &command, Duration::from_secs(2))
            .expect_err("the transaction recheck stays unavailable"),
        HistoryError::Unavailable
    );
}
