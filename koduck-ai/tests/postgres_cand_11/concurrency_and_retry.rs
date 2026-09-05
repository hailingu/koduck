// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-3: concurrency and retry convergence under the measured timing
//! precondition — one winner and typed tip conflicts for competing fresh
//! identities, one identical durable Item for identical retries, distinct
//! increasing sequences for independent chains, and identity drift rejected
//! per dimension (ADR-0004 CA-04 and CA-05).

use std::sync::Arc;
use std::sync::Barrier;
use std::time::{Duration, Instant};

use koduck_ai::adapters::history::postgres::SqlxPostgresExecutor;
use koduck_ai::application::{CorrectionCommand, CorrectionError, CorrectionStore};
use koduck_ai::domain::{Item, ItemId, TenantId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::harness::{Fixture, Harness, command, fresh_fixture, seed_turn};

/// The AC-3 write-attempt deadline every call must stay under.
const DEADLINE: Duration = Duration::from_secs(2);

/// The conservative serialized bound W + 31 * L + S must stay under.
const SERIALIZED_BOUND: Duration = Duration::from_secs(2);

/// One thread's call result with its measured elapsed time.
struct Call {
    result: Result<Item, CorrectionError>,
    elapsed: Duration,
}

pub(crate) fn run() {
    let harness = Harness::connect(40);
    let pool = harness.pool.clone();
    let base = measured_uncontended_latency(&harness, &pool);
    let serialized_bound = base.checked_mul(32).expect("latency bound fits");
    if serialized_bound >= SERIALIZED_BOUND {
        let message = format!(
            "AC-3 timing precondition unavailable: the measured conservative bound \
             W + 31 * L = {serialized_bound:?} reaches the {SERIALIZED_BOUND:?} budget; \
             the arbitration precondition cannot be established on this machine"
        );
        panic!("{message}");
    }

    one_winner_and_typed_conflicts(&harness, base);
    identical_requests_converge(&harness, base);
    independent_chains_receive_distinct_sequences(&harness);
    exact_retry_survives_a_later_successor(&harness);
    identity_drift_rejects_per_dimension(&harness);
    tenants_keep_identities_independent(&harness);
}

/// Measures the conservative per-operation bound used for both W (the
/// winner's lock-held transaction and completion) and L (each remaining
/// caller's lock-held validation), so the precondition is measured rather
/// than assumed.
fn measured_uncontended_latency(harness: &Harness, pool: &PgPool) -> Duration {
    let fixture = fresh_fixture("ac3-precondition");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let mut worst = Duration::ZERO;
    let mut tip = input;
    for attempt in 0..3 {
        let started = Instant::now();
        let admitted = harness
            .correct(command(
                &fixture,
                ItemId::new(),
                tip,
                &format!("precondition {attempt}"),
            ))
            .expect("the uncontended precondition call admits");
        worst = worst.max(started.elapsed());
        tip = admitted.item_id;
    }
    worst
}

/// Spawns `calls` barrier-started threads through the production port and
/// verifies none reaches its deadline.
fn raced(harness: &Harness, commands: Vec<CorrectionCommand>, base: Duration) -> Vec<Call> {
    let barrier = Arc::new(Barrier::new(commands.len()));
    let workers: Vec<_> = commands
        .into_iter()
        .map(|command| {
            let barrier = barrier.clone();
            let executor =
                SqlxPostgresExecutor::new(harness.pool.clone(), harness.runtime.handle().clone());
            std::thread::spawn(move || {
                barrier.wait();
                let started = Instant::now();
                let result = CorrectionStore::correct(&executor, command);
                Call {
                    result,
                    elapsed: started.elapsed(),
                }
            })
        })
        .collect();
    workers
        .into_iter()
        .map(|worker| {
            let call = worker.join().expect("the raced caller joins");
            assert!(
                call.elapsed < DEADLINE,
                "no write attempt may reach its {DEADLINE:?} deadline, took {:?}",
                call.elapsed
            );
            assert!(
                call.elapsed < base * 32 + Duration::from_secs(1),
                "the serialized schedule must stay near the measured bound"
            );
            call
        })
        .collect()
}

fn one_winner_and_typed_conflicts(harness: &Harness, base: Duration) {
    let pool = harness.pool.clone();
    let fixture = fresh_fixture("ac3-one-winner");
    let input = harness
        .runtime
        .block_on(seed_turn(&pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let commands = (0..32)
        .map(|attempt| {
            command(
                &fixture,
                ItemId::new(),
                input,
                &format!("competitor {attempt}"),
            )
        })
        .collect();
    let calls = raced(harness, commands, base);
    let winners: Vec<Item> = calls
        .iter()
        .filter_map(|call| call.result.clone().ok())
        .collect();
    let conflicts = calls
        .iter()
        .filter(|call| call.result == Err(CorrectionError::PredecessorConflict))
        .count();
    assert_eq!(winners.len(), 1, "exactly one fresh identity wins the tip");
    assert_eq!(
        conflicts, 31,
        "every losing fresh identity observes PredecessorConflict"
    );
    let winner = &winners[0];
    assert_eq!(winner.sequence, 2, "the winner takes next_sequence");
    let rows: i64 = harness
        .runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .fetch_one(&pool),
        )
        .expect("read the durable state");
    assert_eq!(rows, 2, "the 31 losers mutate nothing");
    let next_sequence: i64 = harness
        .runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT next_sequence FROM turns \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .fetch_one(&pool),
        )
        .expect("read the durable state");
    assert_eq!(next_sequence, 3, "the counter advances exactly once");
    let replayed = harness.replay(&fixture.tenant, fixture.turn);
    assert_eq!(
        replayed.len(),
        2,
        "raw replay keeps originals plus one winner"
    );
}

fn identical_requests_converge(harness: &Harness, base: Duration) {
    let pool = harness.pool.clone();
    let fixture = fresh_fixture("ac3-identical");
    let input = harness
        .runtime
        .block_on(seed_turn(&pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    let shared = command(&fixture, identity, input, "the same retry");
    let commands = (0..32).map(|_| shared.clone()).collect();
    let calls = raced(harness, commands, base);
    let mut results = calls.into_iter();
    let first = results
        .next()
        .expect("at least one identical call")
        .result
        .expect("every identical request returns the durable item");
    assert_eq!(first.item_id, identity);
    for call in results {
        let converged = call.result.expect("identical calls converge");
        assert_eq!(
            converged, first,
            "every identical request returns the same one durable Item"
        );
    }
    let rows: i64 = harness
        .runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .fetch_one(&pool),
        )
        .expect("read the durable state");
    assert_eq!(rows, 2, "retries write nothing beyond the one item");
}

fn independent_chains_receive_distinct_sequences(harness: &Harness) {
    let pool = harness.pool.clone();
    let fixture = fresh_fixture("ac3-independent");
    harness
        .runtime
        .block_on(seed_turn(&pool, &fixture, "completed", 3, false));
    let first_root = ItemId::from_uuid(Uuid::new_v4());
    let second_root = ItemId::from_uuid(Uuid::new_v4());
    harness.runtime.block_on(harness_seed_two_roots(
        &pool,
        &fixture,
        first_root.as_uuid(),
        second_root.as_uuid(),
    ));
    let commands = vec![
        command(&fixture, ItemId::new(), first_root, "chain one"),
        command(&fixture, ItemId::new(), second_root, "chain two"),
    ];
    let calls = raced(harness, commands, Duration::from_millis(1));
    let mut sequences = Vec::new();
    for call in calls {
        let item = call.result.expect("each independent chain admits");
        sequences.push(item.sequence);
    }
    sequences.sort_unstable();
    assert_eq!(
        sequences,
        vec![3, 4],
        "distinct chains receive distinct increasing sequences"
    );
}

async fn harness_seed_two_roots(pool: &sqlx::PgPool, fixture: &Fixture, first: Uuid, second: Uuid) {
    for (sequence, item_id) in [(1, first), (2, second)] {
        sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             VALUES ($1, $2, $3, $4, $5, 'user_message', '{\"content\":\"r\"}', FALSE, NULL)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(sequence)
        .bind(item_id)
        .execute(pool)
        .await
        .expect("seed an independent chain root");
    }
}

fn exact_retry_survives_a_later_successor(harness: &Harness) {
    let pool = harness.pool.clone();
    let fixture = fresh_fixture("ac3-retry-survivor");
    let input = harness
        .runtime
        .block_on(seed_turn(&pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    let original = harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("the original admission commits");
    let successor = harness
        .correct(command(&fixture, ItemId::new(), original.item_id, "later"))
        .expect("the later successor commits");
    assert!(successor.sequence > original.sequence);
    let before_rows: i64 = harness
        .runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .fetch_one(&pool),
        )
        .expect("read the durable state");
    let retried = harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("the exact retry still returns the original");
    assert_eq!(retried, original, "the retry returns the original Item");
    let after_rows: i64 = harness
        .runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT count(*) FROM turn_items \
         WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .fetch_one(&pool),
        )
        .expect("read the durable state");
    assert_eq!(after_rows, before_rows, "the retry writes nothing");
}

fn identity_drift_rejects_per_dimension(harness: &Harness) {
    let pool = harness.pool.clone();
    let fixture = fresh_fixture("ac3-drift");
    let input = harness
        .runtime
        .block_on(seed_turn(&pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("the original commits");
    for drifted in [
        command(&fixture, identity, input, "different content"),
        command(&fixture, identity, ItemId::new(), "committed"),
    ] {
        assert_eq!(
            harness.correct(drifted),
            Err(CorrectionError::IdentityConflict),
            "every identity-bound field drift must be an IdentityConflict"
        );
    }
}

fn tenants_keep_identities_independent(harness: &Harness) {
    let pool = harness.pool.clone();
    let first = fresh_fixture("ac3-tenant-one");
    let second = Fixture {
        tenant: TenantId::new(format!("cand11-tenant-two-{}", Uuid::new_v4()))
            .expect("valid tenant"),
        subject: first.subject,
        thread: first.thread,
        turn: first.turn,
    };
    let first_input = harness
        .runtime
        .block_on(seed_turn(&pool, &first, "completed", 2, true));
    let shared_identity = ItemId::new();
    let first_input = ItemId::from_uuid(first_input.expect("seeded input item"));
    harness
        .correct(command(&first, shared_identity, first_input, "tenant one"))
        .expect("the first tenant admits the identity");
    // Seed the second tenant with the same Thread/Turn UUIDs but its own
    // rows, so the shared identity admits independently.
    harness
        .runtime
        .block_on(seed_turn(&pool, &second, "completed", 2, true));
    let second_input = harness.runtime.block_on(async {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT item_id FROM turn_items WHERE tenant_id = $1 \
             AND thread_id = $2 AND turn_id = $3 AND sequence = 1",
        )
        .bind(second.tenant.as_str())
        .bind(second.thread.as_uuid())
        .bind(second.turn.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("read the second tenant input identity")
    });
    harness
        .correct(command(
            &second,
            shared_identity,
            ItemId::from_uuid(second_input),
            "tenant two",
        ))
        .expect("the same identity is independent in another tenant");
}
