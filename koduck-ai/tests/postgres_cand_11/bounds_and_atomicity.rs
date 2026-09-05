// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-5: the exact resource bounds are enforced inclusively — 4,096-node
//! chains and 1-MiB stored payloads are admitted, one-over is a
//! `ResourceLimit`, sequence corruption fails closed, and every controlled
//! statement fault and pre-commit cancellation preserves all preexisting
//! rows and the counter (ADR-0004 CA-05, CA-06, and CA-08).

use koduck_ai::application::CorrectionError;
use koduck_ai::domain::{Item, ItemId};

use crate::harness::{
    Harness, assert_unchanged, command, fresh_fixture, install_statement_fault, seed_chain,
    seed_item, seed_turn, snapshot,
};

/// The CA-06 stored-payload read cap under test.
const PAYLOAD_CAP: usize = 1_048_576;

/// The CA-06 ancestor-node admission limit under test.
const NODE_LIMIT: usize = 4_096;

pub(crate) fn run() {
    let harness = Harness::connect(8);
    let pool = harness.pool.clone();
    chain_length_bounds(&harness, &pool);
    stored_payload_bounds(&harness, &pool);
    retry_payload_cap_precedes_decode(&harness, &pool);
    controlled_statement_faults_rollback_atomically(&harness, &pool);
    big_int_ceiling_is_schema_enforced(&harness, &pool);
}

/// A chain of exactly 4,096 valid nodes — predecessor and root included —
/// is admitted; observing a 4,097th node is a `ResourceLimit` with zero
/// mutation.
fn chain_length_bounds(harness: &Harness, pool: &sqlx::PgPool) {
    for (count, admits) in [
        (NODE_LIMIT - 1, true),
        (NODE_LIMIT, true),
        (NODE_LIMIT + 1, false),
    ] {
        let fixture = fresh_fixture("ac5-chain-length");
        harness.runtime.block_on(seed_turn(
            pool,
            &fixture,
            "completed",
            i64::try_from(count).expect("chain count fits i64") + 1,
            false,
        ));
        let chain = harness
            .runtime
            .block_on(seed_chain(pool, &fixture, count, None));
        let tip = ItemId::from_uuid(chain[chain.len() - 1]);
        let before = harness.runtime.block_on(snapshot(pool, &fixture));
        let outcome = harness.correct(command(&fixture, ItemId::new(), tip, "corrected"));
        if admits {
            let admitted: Item = outcome
                .unwrap_or_else(|error| panic!("a {count}-node chain must admit: {error:?}"));
            assert_eq!(admitted.sequence, count as u64 + 1);
            let after = harness.runtime.block_on(snapshot(pool, &fixture));
            assert_eq!(after.next_sequence, before.next_sequence + 1);
            assert_eq!(after.item_rows, before.item_rows + 1);
        } else {
            assert_eq!(
                outcome,
                Err(CorrectionError::ResourceLimit),
                "observing the 4,097th node must be a ResourceLimit"
            );
            let after = harness.runtime.block_on(snapshot(pool, &fixture));
            assert_unchanged(&before, &after);
        }
    }
}

/// A stored ancestor payload of exactly the 1-MiB cap is admitted and one
/// byte over is a `ResourceLimit` whose body is never fetched.
fn stored_payload_bounds(harness: &Harness, pool: &sqlx::PgPool) {
    for (payload_bytes, admits) in [
        (PAYLOAD_CAP - 1, true),
        (PAYLOAD_CAP, true),
        (PAYLOAD_CAP + 1, false),
    ] {
        let fixture = fresh_fixture("ac5-payload-cap");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 3, false));
        let chain = harness
            .runtime
            .block_on(seed_chain(pool, &fixture, 2, Some(payload_bytes)));
        let tip = ItemId::from_uuid(chain[chain.len() - 1]);
        let before = harness.runtime.block_on(snapshot(pool, &fixture));
        let outcome = harness.correct(command(&fixture, ItemId::new(), tip, "corrected"));
        if admits {
            outcome.expect("the within-cap chain admits");
            let after = harness.runtime.block_on(snapshot(pool, &fixture));
            assert_eq!(after.item_rows, before.item_rows + 1);
        } else {
            assert_eq!(
                outcome,
                Err(CorrectionError::ResourceLimit),
                "an oversized ancestor payload must be bounded before its body is fetched"
            );
            let after = harness.runtime.block_on(snapshot(pool, &fixture));
            assert_unchanged(&before, &after);
        }
    }
}

/// The retry-read cap applies to stored correction payloads before any
/// decode or content comparison: an oversized body yields `ResourceLimit`,
/// never a decode outcome.
fn retry_payload_cap_precedes_decode(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac5-retry-cap");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    // The stored body is oversized AND invalid JSON; only the cap can be
    // reported, proving the body was never fetched or decoded.
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        identity.as_uuid(),
        "correction",
        &format!("{{\"content\":\"{}\"", "a".repeat(PAYLOAD_CAP + 8)),
        false,
        Some(input.as_uuid()),
    ));
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(command(&fixture, identity, input, "committed")),
        Err(CorrectionError::ResourceLimit),
        "the stored retry read cap must precede decode and comparison"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}

/// Controlled insert and counter-update failures roll the whole statement
/// group back: no partial insert, no counter advance, and a later retry
/// admits exactly once.
fn controlled_statement_faults_rollback_atomically(harness: &Harness, pool: &sqlx::PgPool) {
    // Insert fault: the new correction row never persists.
    let fixture = fresh_fixture("ac5-insert-fault");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    let insert_fault = harness.runtime.block_on(install_statement_fault(
        pool,
        &fixture,
        "turn_items",
        "INSERT",
    ));
    assert_eq!(
        harness.correct(command(&fixture, identity, input, "faulted insert")),
        Err(CorrectionError::Unavailable),
        "an unexpected insert failure is typed unavailability"
    );
    insert_fault.restore(harness);
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
    let retried = harness
        .correct(command(&fixture, identity, input, "faulted insert"))
        .expect("the retry after the restored insert fault admits");
    assert_eq!(retried.item_id, identity);

    // Counter-update fault: the insert rolls back with the counter update.
    let fixture = fresh_fixture("ac5-counter-fault");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    let update_fault = harness
        .runtime
        .block_on(install_statement_fault(pool, &fixture, "turns", "UPDATE"));
    assert_eq!(
        harness.correct(command(&fixture, identity, input, "faulted counter")),
        Err(CorrectionError::Unavailable),
        "an unexpected counter-update failure is typed unavailability"
    );
    update_fault.restore(harness);
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
    harness
        .correct(command(&fixture, identity, input, "faulted counter"))
        .expect("the retry after the restored counter fault admits");
}

/// The BIGINT ceiling is proven at the schema boundary: the counter can
/// never be nonpositive, so the admission check only defends stale and
/// overflowing values (both covered by AC-2).
fn big_int_ceiling_is_schema_enforced(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac5-schema-ceiling");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    harness.runtime.block_on(async {
        let result = sqlx::query("UPDATE turns SET next_sequence = -1 WHERE tenant_id = $1")
            .bind(fixture.tenant.as_str())
            .execute(pool)
            .await;
        assert!(
            result.is_err(),
            "the production check must reject a negative next_sequence"
        );
        let ceiling: i64 = sqlx::query_scalar(
            "SELECT next_sequence FROM turns \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the counter");
        assert_eq!(ceiling, 2, "the failed update must not mutate the counter");
    });
}
