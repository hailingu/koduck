// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable D-7 attempt-budget boundary checks (ADR-0003 TC-09/TC-12).

use std::sync::{Arc, Barrier};

use koduck_ai::application::{AttemptInsertResolution, AttemptStoreError, ExecutionAttemptStore};
use koduck_ai::domain::execution::{AttemptId, ExactActionBinding};
use koduck_ai::domain::tool::Effect;
use sqlx::postgres::PgPoolOptions;

use super::{
    attempts::{attempt_store, prepared_binding, seed_owner_rows},
    harness,
};

/// Rebuilds one fresh attempt identity under the same canonical Turn owner.
fn sibling(binding: &ExactActionBinding) -> ExactActionBinding {
    ExactActionBinding::new(
        binding.tenant_id().clone(),
        binding.thread_id(),
        binding.turn_id(),
        binding.lease_generation(),
        (binding.profile_id(), binding.profile_version()),
        AttemptId::new(),
        binding.action().clone(),
    )
    .expect("valid sibling binding")
}

#[test]
fn durable_sixteenth_attempt_is_followed_by_the_exact_attempt_limit_rejection() {
    let Some(harness) = harness() else {
        return;
    };
    let first = prepared_binding(Effect::ReadData);
    seed_owner_rows(
        &harness,
        first.tenant_id(),
        first.thread_id(),
        first.turn_id(),
        first.lease_generation(),
    );
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    for slot in 1_u64..=16 {
        let binding = if slot == 1 {
            first.clone()
        } else {
            sibling(&first)
        };
        assert_eq!(
            store.insert_prepared(&binding, slot),
            Ok(AttemptInsertResolution::Inserted),
            "durable attempt {slot} must fit the Turn budget"
        );
    }
    assert_eq!(
        store.insert_prepared(&sibling(&first), 17),
        Err(AttemptStoreError::AttemptLimit),
        "the durable 17th attempt must not be reported as an outage"
    );
}

#[test]
fn concurrent_distinct_inserts_never_exceed_the_sixteen_attempt_budget() {
    // READ COMMITTED takes each statement's snapshot at statement start, so a
    // contender that counted the Turn's attempts before waiting on the Turn
    // lock cannot see rows committed while it waited: the budget check must
    // count on a fresh statement after acquiring the lock, or 32 racing
    // distinct identities can each insert and break the 16-attempt cap
    // (ADR-0003 TC-09, AC-12).
    let Some(harness) = harness() else {
        return;
    };
    let contenders = 32;
    let first = prepared_binding(Effect::ReadData);
    seed_owner_rows(
        &harness,
        first.tenant_id(),
        first.thread_id(),
        first.turn_id(),
        first.lease_generation(),
    );
    let bindings: Vec<ExactActionBinding> = (0..contenders)
        .map(|index| {
            if index == 0 {
                first.clone()
            } else {
                sibling(&first)
            }
        })
        .collect();

    // One connection per contender models independent instances racing the
    // Turn lock. Each contender either inserts, observes the durable limit, or
    // fails closed when the production two-second persistence deadline elapses.
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").expect("harness gated on it");
    let wide_pool = harness
        .runtime
        .block_on(async {
            PgPoolOptions::new()
                .max_connections(u32::try_from(contenders).expect("contender count fits"))
                .connect(&database_url)
                .await
        })
        .expect("wide fixture pool connects");
    let barrier = Arc::new(Barrier::new(contenders));
    let outcomes: Vec<Result<AttemptInsertResolution, AttemptStoreError>> =
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for binding in bindings {
                let mut store = attempt_store(wide_pool.clone(), &harness.runtime);
                let barrier = Arc::clone(&barrier);
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    ExecutionAttemptStore::insert_prepared(&mut store, &binding, 1_000)
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().expect("budget contender completes"))
                .collect()
        });
    let inserted = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(AttemptInsertResolution::Inserted)))
        .count();
    assert!(
        outcomes.iter().all(|outcome| {
            matches!(
                outcome,
                Ok(AttemptInsertResolution::Inserted)
                    | Err(AttemptStoreError::AttemptLimit | AttemptStoreError::Unavailable)
            )
        }),
        "racing durable inserts must either commit, hit the attempt limit, or fail closed"
    );
    let durable_count: i64 = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM tool_execution_attempts \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
            )
            .bind(first.tenant_id().as_str())
            .bind(first.thread_id().as_uuid())
            .bind(first.turn_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("durable budget count is readable");
    assert_eq!(
        i64::try_from(inserted).expect("inserted count fits"),
        durable_count,
        "every inserted resolution corresponds to one durable row"
    );
    assert!(
        durable_count <= 16,
        "the Turn's durable attempt budget must never exceed 16, found {durable_count}"
    );
}
