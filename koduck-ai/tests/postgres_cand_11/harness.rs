// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! Shared fixture, seeding, snapshot, and fault-injection helpers for the
//! CAND-11 `PostgreSQL` acceptance harness.

use std::sync::Once;
use std::sync::mpsc;

use koduck_ai::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, CorrectionCommand, CorrectionError, CorrectionStore, HistoryError, NewItem,
    TurnCommand, TurnHistory,
};
use koduck_ai::domain::{Item, ItemId, TenantId, ThreadId, TrustContext, TurnId};
use std::str::FromStr;

use sqlx::Row;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use uuid::Uuid;

pub(crate) const MIGRATIONS: [&str; 9] = [
    include_str!("../../migrations/0001_cand_1_history.sql"),
    include_str!("../../migrations/0002_cand_2_policy_execution.sql"),
    include_str!("../../migrations/0003_cand_2_requester_ownership.sql"),
    include_str!("../../migrations/0004_cand_2_tool_projections.sql"),
    include_str!("../../migrations/0005_cand_2_execution_attempts.sql"),
    include_str!("../../migrations/0006_cand_2_interrupt_barrier.sql"),
    include_str!("../../migrations/0007_cand_2_tool_audit.sql"),
    include_str!("../../migrations/0008_cand_2_interruption_approval_cancellation.sql"),
    include_str!("../../migrations/0009_cand_3_correction_items.sql"),
];

static MIGRATIONS_ONCE: Once = Once::new();

pub(crate) struct Harness {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) pool: PgPool,
}

impl Harness {
    /// Connects to the isolated test database, failing the acceptance run
    /// explicitly when the prerequisite environment is missing.
    pub(crate) fn connect(max_connections: u32) -> Harness {
        let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").unwrap_or_else(|_| {
            panic!(
                "KODUCK_AI_TEST_DATABASE_URL must point at an isolated PostgreSQL database \
                 with every production migration applicable; without it AC-6 cannot pass"
            )
        });
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("PostgreSQL test runtime");
        let pool = runtime
            .block_on(
                PgPoolOptions::new()
                    .max_connections(max_connections)
                    .connect(&database_url),
            )
            .expect("connect to the disposable test PostgreSQL");
        MIGRATIONS_ONCE.call_once(|| {
            runtime.block_on(async {
                for migration in MIGRATIONS {
                    sqlx::raw_sql(migration)
                        .execute(&pool)
                        .await
                        .expect("apply production migration");
                }
            });
        });
        Harness { runtime, pool }
    }

    pub(crate) fn executor(&self) -> SqlxPostgresExecutor {
        SqlxPostgresExecutor::new(self.pool.clone(), self.runtime.handle().clone())
    }

    pub(crate) fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Admits one correction through the production port. The call is made
    /// from the test thread; the port drives the database on its own runtime.
    pub(crate) fn correct(&self, command: CorrectionCommand) -> Result<Item, CorrectionError> {
        CorrectionStore::correct(&self.executor(), command)
    }

    /// Admits one correction through a caller-supplied pool, so the
    /// corrupt-fixture cases can run the identical production SQL against
    /// the fixture-schema search path.
    pub(crate) fn correct_on(
        &self,
        pool: &PgPool,
        command: CorrectionCommand,
    ) -> Result<Item, CorrectionError> {
        CorrectionStore::correct(
            &SqlxPostgresExecutor::new(pool.clone(), self.handle()),
            command,
        )
    }

    /// Raw ordered replay through the production history port.
    pub(crate) fn replay(&self, tenant: &TenantId, turn: TurnId) -> Vec<Item> {
        let history = PostgresTurnHistory::new(self.executor());
        TurnHistory::replay(&history, tenant, turn).expect("replay succeeds")
    }
}

/// One isolated fixture identity: every seed lives under this tenant, so
/// parallel tests and repeated runs never collide.
#[derive(Clone)]
pub(crate) struct Fixture {
    pub(crate) tenant: TenantId,
    pub(crate) subject: &'static str,
    pub(crate) thread: ThreadId,
    pub(crate) turn: TurnId,
}

pub(crate) fn fresh_fixture(label: &str) -> Fixture {
    Fixture {
        tenant: TenantId::new(format!("cand11-{label}-{}", Uuid::new_v4())).expect("valid tenant"),
        subject: "subject-a",
        thread: ThreadId::new(),
        turn: TurnId::new(),
    }
}

pub(crate) fn trust(fixture: &Fixture) -> TrustContext {
    TrustContext::new(fixture.tenant.clone(), fixture.subject).expect("valid trust context")
}

pub(crate) fn command(
    fixture: &Fixture,
    item_id: ItemId,
    predecessor: ItemId,
    content: &str,
) -> CorrectionCommand {
    CorrectionCommand::new(
        trust(fixture),
        fixture.thread,
        fixture.turn,
        item_id,
        predecessor,
        content,
    )
    .expect("valid correction command")
}

/// Seeds the Thread, one Turn in the requested state, one live lease, and
/// optionally the initial `user_message` Item at sequence 1. Returns the
/// seeded input Item identity when requested.
pub(crate) async fn seed_turn(
    pool: &PgPool,
    fixture: &Fixture,
    status: &str,
    next_sequence: i64,
    with_input_item: bool,
) -> Option<Uuid> {
    sqlx::query(
        "INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.subject)
    .bind(fixture.thread.as_uuid())
    .execute(pool)
    .await
    .expect("seed thread");
    sqlx::query(
        "INSERT INTO turns (tenant_id, thread_id, turn_id, status, next_sequence) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .bind(status)
    .bind(next_sequence)
    .execute(pool)
    .await
    .expect("seed turn");
    sqlx::query(
        "INSERT INTO turn_leases (tenant_id, thread_id, turn_id, generation, \
         renewed_at, expires_at, fenced) \
         VALUES ($1, $2, $3, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP + INTERVAL '1 hour', FALSE)",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .execute(pool)
    .await
    .expect("seed lease");
    let input_item_id = with_input_item.then(Uuid::new_v4);
    if let Some(item_id) = input_item_id {
        seed_item(
            pool,
            fixture,
            1,
            item_id,
            "user_message",
            r#"{"content":"original"}"#,
            false,
            None,
        )
        .await;
    }
    input_item_id
}

/// Inserts one raw `turn_items` row exactly as shaped by the test.
#[allow(
    clippy::too_many_arguments,
    reason = "every column of the raw seed row is an independently shaped fixture input"
)]
pub(crate) async fn seed_item(
    pool: &PgPool,
    fixture: &Fixture,
    sequence: i64,
    item_id: Uuid,
    item_type: &str,
    payload: &str,
    is_terminal: bool,
    corrects: Option<Uuid>,
) {
    sqlx::query(
        "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
         item_type, payload, is_terminal, corrects_item_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .bind(sequence)
    .bind(item_id)
    .bind(item_type)
    .bind(payload)
    .bind(is_terminal)
    .bind(corrects)
    .execute(pool)
    .await
    .expect("seed item");
}

/// Bulk-seeds one linear correction chain: a `user_message` root at sequence
/// 1 followed by `count - 1` corrections, each correcting its predecessor.
/// Returns every chain Item identity from root to tip.
pub(crate) async fn seed_chain(
    pool: &PgPool,
    fixture: &Fixture,
    count: usize,
    root_payload_bytes: Option<usize>,
) -> Vec<Uuid> {
    assert!(count >= 1, "a chain needs at least its root");
    let root_payload = match root_payload_bytes {
        Some(total) => {
            let content = "a".repeat(total - 14);
            format!(r#"{{"content":"{content}"}}"#)
        }
        None => r#"{"content":"original"}"#.to_owned(),
    };
    let root_id = Uuid::new_v4();
    seed_item(
        pool,
        fixture,
        1,
        root_id,
        "user_message",
        &root_payload,
        false,
        None,
    )
    .await;
    let mut identities = vec![root_id];
    while identities.len() < count {
        let segment = (count - identities.len()).min(500);
        let base = i64::try_from(identities.len()).expect("chain fits i64") + 1;
        let mut sequences = Vec::with_capacity(segment);
        let mut ids = Vec::with_capacity(segment);
        let mut targets = Vec::with_capacity(segment);
        for offset in 0..segment {
            let item_id = Uuid::new_v4();
            sequences.push(base + i64::try_from(offset).expect("chain fits i64"));
            ids.push(item_id);
            targets.push(*identities.last().expect("chain predecessor"));
            identities.push(item_id);
        }
        sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             SELECT $1, $2, $3, s, i, 'correction', '{\"content\":\"c\"}', FALSE, t \
             FROM unnest($4::bigint[], $5::uuid[], $6::uuid[]) AS seg(s, i, t)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(sequences)
        .bind(ids)
        .bind(targets)
        .execute(pool)
        .await
        .expect("seed chain segment");
    }
    identities
}

/// The durable Turn state a zero-mutation assertion compares.
pub(crate) struct TurnSnapshot {
    pub(crate) status: String,
    pub(crate) next_sequence: i64,
    pub(crate) item_rows: i64,
    pub(crate) lease_generation: i64,
    pub(crate) lease_fenced: bool,
    pub(crate) interrupt_requested: bool,
    pub(crate) terminal_rows: i64,
}

pub(crate) async fn snapshot(pool: &PgPool, fixture: &Fixture) -> TurnSnapshot {
    let turn = sqlx::query(
        "SELECT t.status, t.next_sequence, t.interrupt_requested, \
         l.generation AS lease_generation, l.fenced AS lease_fenced \
         FROM turns t JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
         WHERE t.tenant_id = $1 AND t.thread_id = $2 AND t.turn_id = $3",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .fetch_one(pool)
    .await
    .expect("read turn snapshot");
    let item_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .fetch_one(pool)
    .await
    .expect("count item rows");
    let terminal_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 AND is_terminal",
    )
    .bind(fixture.tenant.as_str())
    .bind(fixture.thread.as_uuid())
    .bind(fixture.turn.as_uuid())
    .fetch_one(pool)
    .await
    .expect("count terminal rows");
    TurnSnapshot {
        status: turn.get("status"),
        next_sequence: turn.get("next_sequence"),
        item_rows,
        lease_generation: turn.get("lease_generation"),
        lease_fenced: turn.get("lease_fenced"),
        interrupt_requested: turn.get("interrupt_requested"),
        terminal_rows,
    }
}

/// Asserts that the durable Turn state equals the before-snapshot exactly.
pub(crate) fn assert_unchanged(before: &TurnSnapshot, after: &TurnSnapshot) {
    assert_eq!(after.status, before.status, "status changed");
    assert_eq!(
        after.next_sequence, before.next_sequence,
        "next_sequence changed"
    );
    assert_eq!(after.item_rows, before.item_rows, "item rows changed");
    assert_eq!(
        after.lease_generation, before.lease_generation,
        "lease generation changed"
    );
    assert_eq!(
        after.lease_fenced, before.lease_fenced,
        "lease fenced changed"
    );
    assert_eq!(
        after.interrupt_requested, before.interrupt_requested,
        "interrupt flag changed"
    );
    assert_eq!(
        after.terminal_rows, before.terminal_rows,
        "terminal rows changed"
    );
}

/// Derives the stable advisory operation key exactly as the production
/// `commit_reconciliation` helper does from the correction Item identity.
pub(crate) fn advisory_key(item_id: Uuid) -> i64 {
    let bytes = item_id.as_bytes();
    i64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// One held session- or row-level fault on its own connection, released
/// only by [`FaultHandle::release`].
pub(crate) struct FaultHandle {
    locked: mpsc::Receiver<()>,
    release: Option<mpsc::Sender<()>>,
    holder: Option<std::thread::JoinHandle<()>>,
}

impl FaultHandle {
    /// Blocks until the holder connection reports the lock as taken.
    pub(crate) fn wait_until_held(&self) {
        self.locked
            .recv()
            .expect("the fault lock holder reports it is held");
    }

    /// Releases the fault lock and joins the holder connection.
    pub(crate) fn release(mut self) {
        if let Some(release) = self.release.take() {
            release.send(()).expect("signal the fault holder");
        }
        if let Some(holder) = self.holder.take() {
            holder.join().expect("the fault holder connection closes");
        }
    }
}

/// Holds session advisory locks for the given operation keys on a dedicated
/// connection until released.
pub(crate) fn hold_advisory_locks(_harness: &Harness, keys: &[i64]) -> FaultHandle {
    let (locked_sender, locked_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("test database URL");
    let keys = keys.to_vec();
    let holder = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("fault holder runtime");
        runtime.block_on(async {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("fault holder connection");
            for key in keys {
                sqlx::query("SELECT pg_advisory_lock($1)")
                    .bind(key)
                    .execute(&pool)
                    .await
                    .expect("hold advisory fault lock");
            }
            locked_sender.send(()).expect("report the held fault lock");
            let _ = release_receiver.recv();
            sqlx::query("SELECT pg_advisory_unlock_all()")
                .execute(&pool)
                .await
                .expect("release advisory fault locks");
            pool.close().await;
        });
    });
    FaultHandle {
        locked: locked_receiver,
        release: Some(release_sender),
        holder: Some(holder),
    }
}

/// Holds one Turn row lock in an open transaction on a dedicated connection
/// until released.
pub(crate) fn hold_turn_row_lock(_harness: &Harness, fixture: &Fixture) -> FaultHandle {
    let (locked_sender, locked_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("test database URL");
    let tenant = fixture.tenant.as_str().to_owned();
    let thread_id = fixture.thread;
    let turn_id = fixture.turn;
    let holder = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("fault holder runtime");
        runtime.block_on(async {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("fault holder connection");
            let mut transaction = pool.begin().await.expect("fault holder transaction");
            sqlx::query(
                "SELECT turn_id FROM turns \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 FOR UPDATE",
            )
            .bind(&tenant)
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .fetch_one(&mut *transaction)
            .await
            .expect("hold the turn row fault lock");
            locked_sender.send(()).expect("report the held fault lock");
            let _ = release_receiver.recv();
            drop(transaction);
            pool.close().await;
        });
    });
    FaultHandle {
        locked: locked_receiver,
        release: Some(release_sender),
        holder: Some(holder),
    }
}

/// A dedicated pool whose `search_path` prefers an isolated corrupt-fixture
/// schema, so the production SQL observes fixture-only corrupt rows that
/// the production constraints otherwise prevent.
pub(crate) struct CorruptFixture {
    runtime: tokio::runtime::Handle,
    database_url: String,
    pub(crate) pool: PgPool,
    schema: String,
}

impl CorruptFixture {
    /// Creates the isolated schema with a minimal `turn_items` clone that
    /// keeps the primary key and payload column but omits the correction
    /// foreign key, the one-successor index, and the shape constraints.
    pub(crate) fn create(harness: &Harness) -> CorruptFixture {
        let runtime = harness.handle();
        let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("test database URL");
        let schema = format!("cand11_corrupt_{}", Uuid::new_v4().simple());
        runtime.block_on(async {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE SCHEMA {schema}; \
                 CREATE TABLE {schema}.turn_items ( \
                 tenant_id TEXT NOT NULL, thread_id UUID NOT NULL, turn_id UUID NOT NULL, \
                 sequence BIGINT NOT NULL CHECK (sequence > 0), item_id UUID NOT NULL, \
                 item_type TEXT NOT NULL, payload TEXT NOT NULL, \
                 is_terminal BOOLEAN NOT NULL DEFAULT FALSE, \
                 corrects_item_id UUID, \
                 PRIMARY KEY (tenant_id, thread_id, turn_id, sequence))"
            )))
            .execute(&harness.pool)
            .await
            .expect("create the corrupt fixture schema");
        });
        let options = PgConnectOptions::from_str(&database_url)
            .expect("valid test database URL")
            .options([("search_path", format!("{schema},public"))]);
        let pool = runtime
            .block_on(
                PgPoolOptions::new()
                    .max_connections(4)
                    .connect_with(options),
            )
            .expect("connect through the corrupt fixture search path");
        CorruptFixture {
            runtime,
            database_url,
            pool,
            schema,
        }
    }

    /// Seeds one raw row inside the fixture schema without production
    /// constraint protection.
    pub(crate) async fn seed_item(
        &self,
        fixture: &Fixture,
        sequence: i64,
        item_id: Uuid,
        item_type: &str,
        payload: &str,
        corrects: Option<Uuid>,
    ) {
        sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, FALSE, $8)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(sequence)
        .bind(item_id)
        .bind(item_type)
        .bind(payload)
        .bind(corrects)
        .execute(&self.pool)
        .await
        .expect("seed a corrupt fixture row");
    }

    /// Closes the fixture pool and drops the fixture schema, restoring the
    /// fixture-only schema change.
    pub(crate) fn teardown(self) {
        let CorruptFixture {
            runtime,
            database_url,
            pool,
            schema,
        } = self;
        runtime.block_on(async move {
            pool.close().await;
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("reconnect for fixture teardown");
            // The schema name is a generated fixture identifier, never caller input.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE"
            )))
            .execute(&pool)
            .await
            .expect("drop the corrupt fixture schema");
            pool.close().await;
        });
    }
}

/// A tenant-scoped fault trigger that aborts the matching statement group
/// for this fixture's tenant only, restorable by [`StatementFault::restore`].
pub(crate) struct StatementFault {
    pool: PgPool,
    name: String,
    target: &'static str,
}

pub(crate) async fn install_statement_fault(
    pool: &PgPool,
    fixture: &Fixture,
    target: &'static str,
    event: &str,
) -> StatementFault {
    let name = format!("cand11_fault_{}", Uuid::new_v4().simple());
    let tenant = fixture.tenant.as_str();
    // The trigger name is a generated fixture identifier and the tenant is a
    // fixture-owned value; neither is caller input.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {name}_fn() RETURNS trigger AS $body$ \
         BEGIN IF NEW.tenant_id = $tenant${tenant}$tenant$ THEN \
         RAISE EXCEPTION 'cand11 controlled statement fault'; END IF; \
         RETURN NEW; END; $body$ LANGUAGE plpgsql; \
         CREATE TRIGGER {name}_trigger BEFORE {event} ON {target} \
         FOR EACH ROW EXECUTE FUNCTION {name}_fn()"
    )))
    .execute(pool)
    .await
    .expect("install the statement fault trigger");
    StatementFault {
        pool: pool.clone(),
        name,
        target,
    }
}

impl StatementFault {
    /// Drops the fixture-only trigger and function, restoring the schema.
    pub(crate) fn restore(self, harness: &Harness) {
        let StatementFault { pool, name, target } = self;
        harness.runtime.block_on(async move {
            // The trigger and table names are fixture-generated identifiers.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP TRIGGER IF EXISTS {name}_trigger ON {target}; \
                 DROP FUNCTION IF EXISTS {name}_fn()"
            )))
            .execute(&pool)
            .await
            .expect("restore the fixture-only trigger change");
        });
    }
}

/// Drives one complete production foreground Turn to its terminal on a fresh
/// fixture and returns both, so regressions exercise the real lifecycle.
pub(crate) fn foreground_turn_to_terminal(
    harness: &Harness,
    label: &str,
) -> (Fixture, AcceptedTurn) {
    let base = fresh_fixture(label);
    let history = PostgresTurnHistory::new(harness.executor());
    let command = TurnCommand::new(
        trust(&base),
        None,
        "foreground input that must remain untouched",
    )
    .expect("valid foreground command");
    let mut history = history;
    let accepted = TurnHistory::accept_initial(&mut history, &command).expect("accept foreground");
    TurnHistory::append(
        &mut history,
        &accepted,
        NewItem::Terminal(koduck_ai::domain::TerminalOutcome::Cancelled),
    )
    .expect("append the foreground terminal");
    let fixture = Fixture {
        tenant: base.tenant,
        subject: base.subject,
        thread: accepted.thread_id,
        turn: accepted.turn_id,
    };
    (fixture, accepted)
}

/// The CA-09 regression: ordinary foreground append still rejects a
/// terminal Turn.
pub(crate) fn foreground_append_rejected(harness: &Harness, turn: &AcceptedTurn) -> HistoryError {
    let history = PostgresTurnHistory::new(harness.executor());
    let mut history = history;
    TurnHistory::append(
        &mut history,
        turn,
        NewItem::AgentMessageDelta {
            content: "late".to_owned(),
        },
    )
    .expect_err("foreground append must still reject a terminal turn")
}
