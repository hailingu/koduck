// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! AC-0 deferred-`COMMIT` driver preflight for CAND-18 settlement.
//!
//! Proves, on the locked `SQLx` driver, the production Tokio runtime shape,
//! and the disposable `PostgreSQL` fixture, the exact behavior the CAND-18
//! settlement design relies on: a write attempt whose deferred constraint
//! trigger delays `COMMIT` past the production two-second deadline still
//! commits durably, the dropped acknowledgement leaves the backend executing
//! the commit, and a same-pool read-only proof observes the committed marker
//! within its own full two-second budget on every realizable pool-session
//! path. The aborted variant proves a deferred `RAISE EXCEPTION` leaves zero
//! marker rows and no leaked session.
//!
//! The primary pool is built through the same `PgPoolOptions::new()` default
//! construction as `runtime::run` (max 10, min 0, 30 s acquire timeout,
//! 600 s idle timeout, 1,800 s max lifetime, fairness and test-before-acquire
//! enabled, no acquire/release callbacks), adding only the fixture-local
//! `search_path` and application name. No pool option is overridden, so the
//! effective options are identical to the production assembly by
//! construction. The monitor connection lives outside the primary pool so it
//! never consumes a primary-pool candidate session.

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgPool};

/// The exact production per-attempt write deadline (ADR-0001 CAND-1 policy).
const WRITE_ATTEMPT_BUDGET: Duration = Duration::from_secs(2);
/// The exact full read-only proof budget SI-07c reserves for settlement.
const PROOF_BUDGET: Duration = Duration::from_secs(2);
/// Server-side delay the deferred trigger adds to every commit.
const COMMIT_TRIGGER_DELAY: &str = "2.5";
/// Marker identity the writer inserts and every proof reads.
const MARKER_ID: i32 = 42;
/// Marker content a committed delayed transaction must expose to proofs.
const COMMITTED_MARKER: &str = "cand18-commit-survived";
/// Derives the fixture-owned advisory key pair in the transaction-scoped
/// two-`int4` form from the generated schema name, so concurrently running
/// probe fixtures never contend on one key while the writer and its proof
/// within one fixture still share the exact same key.
fn probe_lock_keys(schema: &str) -> (i32, i32) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    schema.hash(&mut hasher);
    let full = hasher.finish();
    let low =
        i32::try_from(u32::try_from(full & 0xFFFF_FFFF).unwrap_or(u32::MAX)).unwrap_or(i32::MAX);
    let high = i32::try_from(u32::try_from(full >> 32).unwrap_or(u32::MAX)).unwrap_or(i32::MAX);
    (low, high)
}

/// The deferred trigger behavior under test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitDelay {
    /// The trigger sleeps, delaying `COMMIT` past the write budget.
    Sleep,
    /// The trigger raises, aborting the commit.
    Abort,
}

/// One isolated probe schema, its production-shaped primary pool, and an
/// independent monitor pool.
struct ProbeFixture {
    database_url: String,
    schema: String,
    lock_keys: (i32, i32),
    primary: PgPool,
    monitor: PgPool,
}

impl ProbeFixture {
    /// Creates the schema, the test-only relation, and the requested deferred
    /// constraint trigger, then connects the primary and monitor pools.
    async fn create(delay: CommitDelay) -> ProbeFixture {
        let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "KODUCK_AI_TEST_DATABASE_URL must point at an isolated disposable \
                 PostgreSQL database; AC-0 has no pass path without its fixture"
            )
        });
        let schema = format!("cand18_commit_probe_{}", uuid::Uuid::new_v4().simple());
        let body = match delay {
            CommitDelay::Sleep => format!("PERFORM pg_sleep({COMMIT_TRIGGER_DELAY});"),
            CommitDelay::Abort => "RAISE EXCEPTION 'cand18 controlled commit fault';".to_owned(),
        };
        // Schema, function, trigger, and relation names are generated fixture
        // identifiers, never caller input.
        let ddl = format!(
            "CREATE SCHEMA {schema}; \
             CREATE TABLE {schema}.probe_markers ( \
             id INT PRIMARY KEY, marker TEXT NOT NULL); \
             CREATE FUNCTION {schema}.probe_commit_trigger() RETURNS trigger AS \
             $body$ BEGIN {body} RETURN NEW; END; $body$ LANGUAGE plpgsql; \
             CREATE CONSTRAINT TRIGGER probe_commit_deferred \
             AFTER INSERT ON {schema}.probe_markers \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW \
             EXECUTE FUNCTION {schema}.probe_commit_trigger()"
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("fixture admin connection");
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&admin)
            .await
            .expect("create the probe fixture schema");
        admin.close().await;
        // The primary pool keeps the exact production PgPoolOptions::new()
        // defaults and adds only the fixture search_path plus an
        // application_name, as AC-0 permits.
        let options = PgConnectOptions::from_str(&database_url)
            .expect("valid test database URL")
            .options([
                ("search_path", schema.clone()),
                (
                    "application_name",
                    "koduck_cand18_commit_fault_probe".to_owned(),
                ),
            ]);
        let primary = PgPoolOptions::new()
            .connect_with(options)
            .await
            .expect("connect the production-shaped primary pool");
        let monitor = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect the independent monitor pool");
        ProbeFixture {
            database_url,
            schema: schema.clone(),
            lock_keys: probe_lock_keys(&schema),
            primary,
            monitor,
        }
    }

    /// Asserts the resolved trigger and relation OIDs belong to this fixture
    /// schema, then closes the pools and drops only the generated schema.
    async fn teardown(self) {
        let ProbeFixture {
            database_url,
            schema,
            lock_keys: _lock_keys,
            primary,
            monitor,
        } = self;
        async move {
            let owned: (i64, i64) = sqlx::query_as(
                "SELECT \
                 (SELECT count(*) FROM pg_class c JOIN pg_namespace n \
                 ON c.relnamespace = n.oid \
                 WHERE n.nspname = $1 AND c.relname = 'probe_markers'), \
                 (SELECT count(*) FROM pg_trigger t JOIN pg_class c \
                 ON t.tgrelid = c.oid JOIN pg_namespace n ON c.relnamespace = n.oid \
                 WHERE n.nspname = $1 AND c.relname = 'probe_markers' \
                 AND t.tgname = 'probe_commit_deferred')",
            )
            .bind(&schema)
            .fetch_one(&monitor)
            .await
            .expect("read fixture relation ownership");
            assert_eq!(
                owned,
                (1, 1),
                "the probe relation and deferred trigger must resolve inside the fixture schema"
            );
            primary.close().await;
            monitor.close().await;
            let admin = PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("reconnect for fixture teardown");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE"
            )))
            .execute(&admin)
            .await
            .expect("drop the probe fixture schema");
            let leftover: i64 =
                sqlx::query_scalar("SELECT count(*) FROM pg_namespace WHERE nspname = $1")
                    .bind(&schema)
                    .fetch_one(&admin)
                    .await
                    .expect("confirm fixture schema removal");
            assert_eq!(leftover, 0, "no fixture schema may remain");
            admin.close().await;
        }
        .await;
    }
}

/// Records one backend's identity and keeps its session checked out of the
/// primary pool until the returned guard drops.
async fn hold_session(pool: &PgPool) -> (sqlx::pool::PoolConnection<sqlx::Postgres>, i32) {
    let mut connection = pool.acquire().await.expect("hold a primary session");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .expect("read the held session backend pid");
    (connection, pid)
}

/// Reports whether the writer backend currently waits inside the deferred
/// trigger's `pg_sleep`, proving the server reached `COMMIT` execution.
async fn observe_commit_sleep(monitor: &PgPool, writer_pid: i32) -> bool {
    let state: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT state::text, wait_event::text FROM pg_stat_activity WHERE pid = $1")
            .bind(writer_pid)
            .fetch_optional(monitor)
            .await
            .expect("observe the writer backend");
    matches!(state, Some((_, Some(wait))) if wait == "PgSleep")
}

/// Runs the writer attempt concurrently with its monitor and returns once the
/// attempt ended without a same-attempt acknowledgement. For [`CommitDelay::Sleep`]
/// the attempt is cancelled by the production write budget; for
/// [`CommitDelay::Abort`] the server-side commit itself fails. Returns the
/// writer backend PID.
async fn run_write_attempt(fixture: &ProbeFixture, delay: CommitDelay) -> i32 {
    let mut writer = fixture
        .primary
        .acquire()
        .await
        .expect("writer acquires a session");
    let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *writer)
        .await
        .expect("read the writer backend pid");
    let write = async {
        let mut transaction = writer.begin().await.expect("writer transaction");
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(fixture.lock_keys.0)
            .bind(fixture.lock_keys.1)
            .execute(&mut *transaction)
            .await
            .expect("writer takes the probe advisory key");
        sqlx::query("INSERT INTO probe_markers (id, marker) VALUES ($1, $2)")
            .bind(MARKER_ID)
            .bind(COMMITTED_MARKER)
            .execute(&mut *transaction)
            .await
            .expect("writer inserts the marker");
        transaction.commit().await
    };
    let started = std::time::Instant::now();
    let monitor = async {
        match delay {
            // The monitor must observe the deferred trigger before the write
            // deadline cancels the acknowledgement.
            CommitDelay::Sleep => loop {
                if observe_commit_sleep(&fixture.monitor, writer_pid).await {
                    break;
                }
                assert!(
                    started.elapsed() < WRITE_ATTEMPT_BUDGET,
                    "the writer must reach the deferred trigger before its write deadline"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            },
            // Give the aborting commit a moment to surface its server-side error.
            CommitDelay::Abort => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        std::time::Instant::now()
    };
    let (attempted, trigger_observed_at) =
        tokio::join!(tokio::time::timeout(WRITE_ATTEMPT_BUDGET, write), monitor);
    assert!(
        trigger_observed_at.duration_since(started) < WRITE_ATTEMPT_BUDGET,
        "the monitor must complete inside the write budget"
    );
    match attempted {
        // The delayed commit cannot acknowledge inside the production budget.
        Err(_elapsed) => {
            assert_eq!(delay, CommitDelay::Sleep, "unexpected write task hang");
        }
        // The abort variant fails its server-side commit inside the budget.
        Ok(Err(_)) => {
            assert_eq!(delay, CommitDelay::Abort, "unexpected write task failure");
        }
        Ok(Ok(commit)) => {
            panic!("the probe write attempt must never acknowledge success: {commit:?}");
        }
    }
    drop(writer);
    writer_pid
}

/// What one proof read: an absent row means nothing from the attempt committed.
struct ProofOutcome {
    marker: Option<String>,
    proof_pid: Option<i32>,
}

/// Runs the read-only proof through the primary pool under the same advisory
/// key, asserting the committed marker inside the full proof budget.
async fn run_proof(
    fixture: &ProbeFixture,
    expected_marker: Option<&str>,
) -> (ProofOutcome, Duration) {
    let started = std::time::Instant::now();
    let proof = tokio::time::timeout(PROOF_BUDGET, async {
        let mut connection = fixture
            .primary
            .acquire()
            .await
            .expect("proof acquires from the primary pool");
        let mut transaction = connection.begin().await.expect("proof transaction");
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(fixture.lock_keys.0)
            .bind(fixture.lock_keys.1)
            .execute(&mut *transaction)
            .await
            .expect("proof takes the probe advisory key");
        let row: Option<(Option<String>, i32)> =
            sqlx::query_as("SELECT marker, pg_backend_pid() FROM probe_markers WHERE id = $1")
                .bind(MARKER_ID)
                .fetch_optional(&mut *transaction)
                .await
                .expect("proof reads the marker");
        row
    })
    .await
    .expect("proof completes within its full budget including acquire and lock wait");
    let outcome = match proof {
        Some((marker, proof_pid)) => ProofOutcome {
            marker,
            proof_pid: Some(proof_pid),
        },
        None => ProofOutcome {
            marker: None,
            proof_pid: None,
        },
    };
    assert_eq!(
        outcome.marker.as_deref(),
        expected_marker,
        "the proof must observe the exact committed marker state"
    );
    let elapsed = started.elapsed();
    (outcome, elapsed)
}

/// Asserts every primary session is reusable after the held guards drop.
async fn assert_no_leaked_sessions(fixture: &ProbeFixture) {
    for _ in 0..10 {
        let mut connection =
            tokio::time::timeout(Duration::from_secs(5), fixture.primary.acquire())
                .await
                .expect("no primary session may stay leaked")
                .expect("acquire a released session");
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&mut *connection)
            .await
            .expect("the released session responds");
    }
}

/// AC-0 same-backend path: with every other session held, the proof reuses
/// the writer's own backend after its `COMMIT` and queued rollback cleanup.
pub(crate) fn run_same_backend() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("probe runtime");
    runtime.block_on(async {
        let fixture = ProbeFixture::create(CommitDelay::Sleep).await;
        let mut held = Vec::new();
        for _ in 0..9 {
            held.push(hold_session(&fixture.primary).await);
        }
        let writer_pid = run_write_attempt(&fixture, CommitDelay::Sleep).await;
        let (proof, elapsed) = run_proof(&fixture, Some(COMMITTED_MARKER)).await;
        assert_eq!(
            proof.marker.as_deref(),
            Some(COMMITTED_MARKER),
            "the timed-out commit must survive"
        );
        assert_eq!(
            proof.proof_pid,
            Some(writer_pid),
            "the same-backend path must prove on the writer's own backend"
        );
        assert!(
            elapsed <= PROOF_BUDGET,
            "the proof used {elapsed:?} of its full {PROOF_BUDGET:?} budget"
        );
        drop(held);
        assert_no_leaked_sessions(&fixture).await;
        fixture.teardown().await;
    });
}

/// AC-0 different-backend path: one non-writer session is released for the
/// proof while the writer commit is still blocked, so the proof waits for the
/// advisory key on a different backend and reads the same committed marker.
pub(crate) fn run_other_backend() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("probe runtime");
    runtime.block_on(async {
        let fixture = ProbeFixture::create(CommitDelay::Sleep).await;
        let mut held = Vec::new();
        for _ in 0..9 {
            held.push(hold_session(&fixture.primary).await);
        }
        let writer_pid = run_write_attempt(&fixture, CommitDelay::Sleep).await;
        // The attempt ended; the writer commit is still inside the deferred
        // trigger. Re-confirm the block, then release exactly one known
        // non-writer session as the proof's only idle candidate.
        assert!(
            observe_commit_sleep(&fixture.monitor, writer_pid).await,
            "the writer commit must still be blocked when the proof session is released"
        );
        drop(held.remove(0));
        let (proof, elapsed) = run_proof(&fixture, Some(COMMITTED_MARKER)).await;
        assert_eq!(
            proof.marker.as_deref(),
            Some(COMMITTED_MARKER),
            "the timed-out commit must survive"
        );
        assert_ne!(
            proof.proof_pid,
            Some(writer_pid),
            "the different-backend path must prove on the released session"
        );
        assert!(
            elapsed <= PROOF_BUDGET,
            "the proof used {elapsed:?} of its full {PROOF_BUDGET:?} budget"
        );
        drop(held);
        assert_no_leaked_sessions(&fixture).await;
        fixture.teardown().await;
    });
}

/// AC-0 aborted variant: a deferred `RAISE EXCEPTION` leaves zero marker rows
/// and no leaked primary session.
pub(crate) fn run_aborted_commit() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("probe runtime");
    runtime.block_on(async {
        let fixture = ProbeFixture::create(CommitDelay::Abort).await;
        let mut held = Vec::new();
        for _ in 0..9 {
            held.push(hold_session(&fixture.primary).await);
        }
        let _writer_pid = run_write_attempt(&fixture, CommitDelay::Abort).await;
        let (proof, _elapsed) = run_proof(&fixture, None).await;
        assert!(
            proof.proof_pid.is_none(),
            "the aborted attempt must leave no committed marker row"
        );
        drop(held);
        assert_no_leaked_sessions(&fixture).await;
        fixture.teardown().await;
    });
}
