// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-4: settlement is bounded and truthful — real `PostgreSQL` stalls at the
//! identity lock, the Turn row lock, and under a deliberately
//! deadline-exhausted 32-writer case, plus wrong-owner lookups, caller
//! connection loss, and exact-retry deduplication after every ambiguous
//! acknowledgement (ADR-0004 CA-07 and CA-08).
//!
//! The exact 2-second budget arithmetic is proven separately by the
//! deterministic `correction_settlement_budget` unit tests in the
//! production module; this harness proves the real-boundary outcomes.

use std::str::FromStr;
use std::sync::Arc;
use std::sync::Barrier;
use std::time::{Duration, Instant};

use koduck_ai::adapters::history::postgres::SqlxPostgresExecutor;
use koduck_ai::application::{CorrectionCommand, CorrectionError, CorrectionStore};
use koduck_ai::domain::{Item, ItemId};
use uuid::Uuid;

use crate::harness::{
    self, Fixture, Harness, advisory_key, assert_unchanged, command, fresh_fixture, seed_item,
    seed_turn, snapshot,
};

pub(crate) fn run() {
    let harness = Harness::connect(40);
    let pool = harness.pool.clone();
    identity_lock_stall_is_bounded_and_unknown(&harness, &pool);
    turn_row_stall_proves_absence(&harness, &pool);
    committed_exact_match_succeeds_during_a_stall(&harness, &pool);
    wrong_owner_lookup_is_not_found(&harness, &pool);
    deadline_exhausted_writers_stay_unknown_and_unique(&harness, &pool);
}

/// The identity lock is held by a foreign session: the write attempt and its
/// one reconciliation each consume their full budget, the outcome is
/// `Unavailable` with unknown commit state, nothing mutates, and a later
/// exact retry admits exactly once.
fn identity_lock_stall_is_bounded_and_unknown(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac4-identity-stall");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    let fault = harness::hold_advisory_locks(harness, &[advisory_key(identity.as_uuid())]);
    fault.wait_until_held();
    let started = Instant::now();
    assert_eq!(
        harness.correct(command(&fixture, identity, input, "stalled")),
        Err(CorrectionError::Unavailable),
        "a stalled identity lock resolves to the unknown outcome"
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_secs(2),
        "both budgets must be consumed before giving up, took {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "the bounded settlement must not wait unboundedly, took {elapsed:?}"
    );
    fault.release();
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    let admitted = harness
        .correct(command(&fixture, identity, input, "stalled"))
        .expect("the released retry admits");
    assert_eq!(admitted.item_id, identity);
    let retried = harness
        .correct(command(&fixture, identity, input, "stalled"))
        .expect("the exact retry deduplicates");
    assert_eq!(retried, admitted, "no duplicate may be created on retry");
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(after.item_rows, before.item_rows + 1);
}

/// The Turn row is locked by a foreign transaction and the writer session
/// carries a short `lock_timeout`, so the blocked write attempt fails fast,
/// rolls back, and releases its identity lock; one read-only reconciliation
/// then proves absence (`NotApplied`) and the Turn stays unchanged
/// (CA-07 and CA-08).
fn turn_row_stall_proves_absence(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac4-turn-stall");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    let fault = harness::hold_turn_row_lock(harness, &fixture);
    fault.wait_until_held();
    let outcome = terminated_writer_call(
        harness,
        pool,
        &fixture,
        &command(&fixture, ItemId::new(), input, "stalled"),
    );
    assert_eq!(
        outcome,
        Err(CorrectionError::NotApplied),
        "a proven absence after the settled writer is NotApplied"
    );
    fault.release();
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
    assert!(
        harness
            .correct(command(&fixture, ItemId::new(), input, "after release"))
            .is_ok(),
        "the pool and Turn recover after the cancelled attempt"
    );
}

/// While the Turn row lock fails the write attempt under a short
/// `lock_timeout`, one exact match is already durable; the read-only
/// reconciliation observes the committed match and returns its durable Item
/// without any duplicate write (CA-07).
fn committed_exact_match_succeeds_during_a_stall(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac4-committed-stall");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        identity.as_uuid(),
        "correction",
        "{\"content\":\"seeded\"}",
        false,
        Some(input.as_uuid()),
    ));
    let fault = harness::hold_turn_row_lock(harness, &fixture);
    fault.wait_until_held();
    let outcome = terminated_writer_call(
        harness,
        pool,
        &fixture,
        &command(&fixture, identity, input, "seeded"),
    );
    let reconciled = outcome.expect("the reconciliation observes the committed exact match");
    assert_eq!(reconciled.item_id, identity);
    assert_eq!(reconciled.sequence, 2);
    fault.release();
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        after.item_rows, 2,
        "the stalled writer must not have added a duplicate"
    );
}

/// Runs one correction through a dedicated labeled session that is blocked
/// on the faulted Turn row, then terminates that backend, so the write
/// attempt ends in a real transport loss: the bounded reconciliation
/// decides the outcome (ADR-0004 CA-07).
fn terminated_writer_call(
    harness: &Harness,
    pool: &sqlx::PgPool,
    _fixture: &Fixture,
    lost_command: &CorrectionCommand,
) -> Result<Item, CorrectionError> {
    let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
    let caller_label = format!("cand11-terminate-{}", Uuid::new_v4().simple());
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("test database URL");
    let label = caller_label.clone();
    let lost_command = lost_command.clone();
    let caller = std::thread::spawn(move || {
        // The executor drives the database through its runtime handle, so
        // the caller runtime must be multi-thread for Handle::block_on.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("terminated caller runtime");
        let options = sqlx::postgres::PgConnectOptions::from_str(&database_url)
            .expect("valid test database URL")
            .application_name(&label);
        let pool = runtime
            .block_on(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(2)
                    .connect_with(options),
            )
            .expect("the terminated caller pool");
        ready_sender.send(()).expect("report the caller pool");
        let executor = SqlxPostgresExecutor::new(pool, runtime.handle().clone());
        CorrectionStore::correct(&executor, lost_command)
    });
    ready_receiver
        .recv()
        .expect("the terminated caller pool is ready");
    std::thread::sleep(Duration::from_millis(400));
    harness.runtime.block_on(async {
        sqlx::query(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = current_database() AND application_name = $1",
        )
        .bind(&caller_label)
        .execute(pool)
        .await
        .expect("terminate the blocked writer backend");
    });
    caller.join().expect("the terminated caller joins")
}

/// A wrong-owner caller cannot reconcile or even observe the stored
/// identity: missing and non-owned targets are an indistinguishable
/// `NotFound` with zero mutation.
fn wrong_owner_lookup_is_not_found(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac4-wrong-owner");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        identity.as_uuid(),
        "correction",
        "{\"content\":\"owned\"}",
        false,
        Some(input.as_uuid()),
    ));
    let impostor_trust = koduck_ai::domain::TrustContext::new(fixture.tenant.clone(), "subject-b")
        .expect("valid trust context");
    let impostor = CorrectionCommand::new(
        impostor_trust,
        fixture.thread,
        fixture.turn,
        identity,
        input,
        "owned",
    )
    .expect("valid command shape");
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(impostor),
        Err(CorrectionError::NotFound),
        "a wrong-owner retry must be indistinguishable from a missing target"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}

/// Thirty-two writers blocked behind held identity locks each consume both
/// bounded attempts, stay `Unavailable`, mutate nothing, and deduplicate
/// once admitted after release.
fn deadline_exhausted_writers_stay_unknown_and_unique(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac4-deadline-32");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identities: Vec<ItemId> = (0..32).map(|_| ItemId::new()).collect();
    let keys: Vec<i64> = identities
        .iter()
        .map(|identity| advisory_key(identity.as_uuid()))
        .collect();
    let fault = harness::hold_advisory_locks(harness, &keys);
    fault.wait_until_held();
    let barrier = Arc::new(Barrier::new(identities.len()));
    let commands: Vec<CorrectionCommand> = identities
        .iter()
        .map(|identity| command(&fixture, *identity, input, "exhausted"))
        .collect();
    let workers: Vec<_> = commands
        .into_iter()
        .map(|command| {
            let barrier = barrier.clone();
            let executor =
                SqlxPostgresExecutor::new(pool.clone(), harness.runtime.handle().clone());
            std::thread::spawn(move || {
                barrier.wait();
                let started = Instant::now();
                let result = CorrectionStore::correct(&executor, command);
                (result, started.elapsed())
            })
        })
        .collect();
    for worker in workers {
        let (result, elapsed) = worker.join().expect("the exhausted writer joins");
        assert_eq!(
            result,
            Err(CorrectionError::Unavailable),
            "a deadline-exhausted writer must report the unknown outcome"
        );
        assert!(
            elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(8),
            "each exhausted writer must consume its bounded budgets, took {elapsed:?}"
        );
    }
    fault.release();
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        before.next_sequence, 2,
        "no exhausted writer may have allocated a sequence"
    );
    let admitted = harness
        .correct(command(&fixture, identities[0], input, "exhausted"))
        .expect("the first released retry admits");
    assert_eq!(admitted.item_id, identities[0]);
    let deduplicated = harness
        .correct(command(&fixture, identities[0], input, "exhausted"))
        .expect("the exact retry deduplicates");
    assert_eq!(deduplicated, admitted);
    assert_eq!(
        harness.correct(command(&fixture, identities[1], input, "exhausted")),
        Err(CorrectionError::PredecessorConflict),
        "the remaining exhausted identities observe the tip conflict"
    );
}
