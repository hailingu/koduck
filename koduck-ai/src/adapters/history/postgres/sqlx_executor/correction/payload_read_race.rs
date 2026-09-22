// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! CA-06: a concurrent committed payload growth cannot bypass a prior size
//! check. A fixture-only `PostgreSQL` view pauses projection of the old value;
//! two real connections then exercise the unchanged transaction entry points.

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use uuid::Uuid;

use crate::application::{CorrectionCommand, CorrectionError};
use crate::domain::{Item, ItemId, TenantId, ThreadId, TurnId};

use super::commit_ack_loss::{
    assert_durable_state, connected_pool, correction_command, seed_completed_turn, seeded_input_id,
};
use super::{
    MAX_STORED_PAYLOAD_BYTES, STORED_PAYLOAD_SQL, STREAMED_ANCESTRY_SQL, WriteFailure,
    correct_async, reconcile_async,
};

/// Selects an actual admission entry point and the payload that changes.
#[derive(Clone, Copy, Debug)]
enum ReadCase {
    Retry,
    Reconcile,
    NewAncestor,
    RetryAncestor,
    ReconcileAncestor,
}

/// Owns one tiny production-shaped history and its fixture-only projection.
struct Fixture {
    tenant: TenantId,
    thread: ThreadId,
    turn: TurnId,
    command: CorrectionCommand,
    target: Uuid,
    schema: String,
    key: i64,
    reader: PgPool,
}

/// The old precheck snapshot is released only after another connection commits
/// an oversized value; rejection and subsequent recovery must both be truthful.
#[test]
fn payload_growth_between_statements_is_bounded() {
    let (runtime, pool) = connected_pool();
    let _permit = runtime.block_on(crate::test_migrations::reserve_database());
    for case in [
        ReadCase::Retry,
        ReadCase::Reconcile,
        ReadCase::NewAncestor,
        ReadCase::RetryAncestor,
        ReadCase::ReconcileAncestor,
    ] {
        let fixture = runtime.block_on(Fixture::create(&pool, case));
        let outcome = runtime.block_on(grow_during_precheck(&pool, &fixture, case));
        runtime.block_on(fixture.remove_projection(&pool));
        assert_durable_state(
            &runtime,
            &pool,
            &fixture.tenant,
            fixture.thread,
            fixture.turn,
            2,
            3,
        );
        assert_eq!(outcome, Err(CorrectionError::ResourceLimit), "{case:?}");
        let recovered = runtime
            .block_on(read(&pool, fixture.command.clone(), case))
            .expect("restoring the bounded payload makes the operation lawful again");
        assert_eq!(recovered.item_id, fixture.command.item_id());
        let rows = if matches!(case, ReadCase::NewAncestor) {
            3
        } else {
            2
        };
        assert_durable_state(
            &runtime,
            &pool,
            &fixture.tenant,
            fixture.thread,
            fixture.turn,
            rows,
            rows + 1,
        );
    }
    runtime.block_on(pool.close());
}

impl Fixture {
    /// Seeds through the existing production fixture and a real admitted correction.
    async fn create(pool: &PgPool, case: ReadCase) -> Self {
        let tenant =
            TenantId::new(format!("cand11-read-race-{}", Uuid::new_v4())).expect("fixture tenant");
        let thread = ThreadId::new();
        let turn = TurnId::new();
        seed_completed_turn(pool, &tenant, &thread, &turn).await;
        let root = seeded_input_id(pool, &tenant, &thread, &turn).await;
        let identity = ItemId::new();
        let original = correction_command(&tenant, thread, turn, identity, root);
        read(pool, original.clone(), ReadCase::Retry)
            .await
            .expect("admit the original correction");
        let command = if matches!(case, ReadCase::NewAncestor) {
            correction_command(&tenant, thread, turn, ItemId::new(), identity)
        } else {
            original
        };
        let target = if matches!(case, ReadCase::Retry | ReadCase::Reconcile) {
            identity.as_uuid()
        } else {
            root.as_uuid()
        };
        let schema = format!("cand11_read_race_{}", Uuid::new_v4().simple());
        let key = i64::from_ne_bytes(*Uuid::new_v4().as_bytes().first_chunk().expect("key bytes"));
        let reader = create_projection(pool, &schema, target, key).await;
        Self {
            tenant,
            thread,
            turn,
            command,
            target,
            schema,
            key,
            reader,
        }
    }

    /// Drops the fixture-only view after the operation and writer have both settled.
    async fn remove_projection(&self, pool: &PgPool) {
        self.reader.close().await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(pool)
        .await
        .expect("remove fixture projection");
    }
}

/// Projects the old snapshot payload through an advisory barrier. All metadata,
/// constraints, and the writer's rows remain in the production migrated tables.
async fn create_projection(pool: &PgPool, schema: &str, target: Uuid, key: i64) -> PgPool {
    // Every interpolated identifier/value here is generated by this fixture.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {schema}; \
         CREATE FUNCTION {schema}.paused_payload(value TEXT) RETURNS TEXT \
         LANGUAGE plpgsql VOLATILE AS $body$ BEGIN \
         PERFORM pg_advisory_xact_lock({key}); RETURN value; END $body$; \
         CREATE VIEW {schema}.turn_items AS SELECT \
         tenant_id, thread_id, turn_id, sequence, item_id, item_type, \
         CASE WHEN item_id = '{target}'::uuid \
              THEN {schema}.paused_payload(payload) ELSE payload END AS payload, \
         is_terminal, corrects_item_id FROM public.turn_items"
    )))
    .execute(pool)
    .await
    .expect("create a fixture-only read barrier");
    let url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("disposable database");
    let options = PgConnectOptions::from_str(&url)
        .expect("database options")
        .options([("search_path", format!("{schema},public"))]);
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("one reader connection")
}

/// Runs the production transaction or its read-only reconciliation with the
/// same error mapping used by the public correction port.
async fn read(
    pool: &PgPool,
    command: CorrectionCommand,
    case: ReadCase,
) -> Result<Item, CorrectionError> {
    if matches!(case, ReadCase::Reconcile | ReadCase::ReconcileAncestor) {
        return reconcile_async(pool, command)
            .await
            .map(|item| item.expect("the fixture correction exists"));
    }
    correct_async(pool, command)
        .await
        .map_err(|error| match error {
            WriteFailure::Resolved(error) => error,
            WriteFailure::Ambiguous => CorrectionError::Unavailable,
        })
}

/// Coordinates one reader and one writer, then restores only the writer's own
/// payload change before returning the observed admission outcome.
async fn grow_during_precheck(
    pool: &PgPool,
    fixture: &Fixture,
    case: ReadCase,
) -> Result<Item, CorrectionError> {
    let mut writer = pool.acquire().await.expect("writer connection");
    let original: String =
        sqlx::query_scalar("SELECT payload FROM turn_items WHERE tenant_id = $1 AND item_id = $2")
            .bind(fixture.tenant.as_str())
            .bind(fixture.target)
            .fetch_one(&mut *writer)
            .await
            .expect("original payload");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&fixture.reader)
        .await
        .expect("reader identity");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(fixture.key)
        .execute(&mut *writer)
        .await
        .expect("hold the read barrier");
    let writer_step = async {
        wait_for_barrier(&mut writer, pid).await;
        sqlx::query(
            "UPDATE turn_items SET payload = repeat('x', $3) WHERE tenant_id = $1 AND item_id = $2",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.target)
        .bind(i32::try_from(MAX_STORED_PAYLOAD_BYTES + 1).expect("fixture size"))
        .execute(&mut *writer)
        .await
        .expect("commit concurrent payload growth");
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(fixture.key)
            .execute(&mut *writer)
            .await
            .expect("release the old-value projection");
    };
    let (outcome, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            read(&fixture.reader, fixture.command.clone(), case),
            writer_step
        )
    })
    .await
    .expect("the deterministic two-connection race settles");
    assert_bounded_projection(&mut writer, fixture).await;
    sqlx::query("UPDATE turn_items SET payload = $3 WHERE tenant_id = $1 AND item_id = $2")
        .bind(fixture.tenant.as_str())
        .bind(fixture.target)
        .bind(original)
        .execute(&mut *writer)
        .await
        .expect("restore the writer's fixture change");
    outcome
}

/// Checks SQL result values themselves: no oversized body crosses the driver
/// boundary, even if the decoder would otherwise reject it after allocation.
async fn assert_bounded_projection(writer: &mut sqlx::PgConnection, fixture: &Fixture) {
    let payload: Option<String> = sqlx::query_scalar(STORED_PAYLOAD_SQL)
        .bind(fixture.tenant.as_str())
        .bind(fixture.target)
        .bind(MAX_STORED_PAYLOAD_BYTES)
        .fetch_one(&mut *writer)
        .await
        .expect("bounded retry projection");
    assert!(
        payload.is_none(),
        "the oversized body must not reach the driver"
    );
    let rows =
        sqlx::query_as::<_, (Uuid, Option<Uuid>, String, Option<String>)>(STREAMED_ANCESTRY_SQL)
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .bind(fixture.command.predecessor_item_id().as_uuid())
            .bind(MAX_STORED_PAYLOAD_BYTES)
            .fetch_all(&mut *writer)
            .await
            .expect("bounded ancestor projection for the one/two-node fixture");
    for (identity, _, _, payload) in rows {
        if identity == fixture.target {
            assert!(
                payload.is_none(),
                "oversized ancestor body crossed the driver"
            );
        } else {
            let cap = usize::try_from(MAX_STORED_PAYLOAD_BYTES).expect("payload cap fits usize");
            assert!(payload.is_some_and(|body| body.len() <= cap));
        }
    }
}

/// Observes the actual `PostgreSQL` wait state, never a guessed scheduling sleep.
async fn wait_for_barrier(writer: &mut sqlx::PgConnection, reader_pid: i32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid = $1 \
                 AND locktype = 'advisory' AND NOT granted)",
            )
            .bind(reader_pid)
            .fetch_one(&mut *writer)
            .await
            .expect("barrier state");
            if waiting {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("reader reached the size-check snapshot");
}
