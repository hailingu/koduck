// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! CA-03/CA-04: exact retries reject invalid durable ancestry without writes.

use koduck_ai::application::CorrectionError;
use koduck_ai::domain::ItemId;
use sqlx::PgPool;
use uuid::Uuid;

use crate::harness::{
    CorruptFixture, Fixture, Harness, assert_unchanged, command, fresh_fixture, seed_chain,
    seed_item, seed_turn, snapshot,
};

/// Covers production-permitted payload corruption and constraint-free link corruption.
pub(crate) fn run() {
    let harness = Harness::connect(2);
    schema_permitted_ancestry(&harness);
    misordered_sole_successor(&harness);
    terminal_message_roots_fail_closed(&harness);
    corrupt_links(&harness);
}

/// A sole direct successor with an earlier sequence is corrupt even though
/// the stored correction and its own ancestors are ordered (CA-03/CA-04).
fn misordered_sole_successor(harness: &Harness) {
    let fixture = fresh_fixture("retry-misordered-successor");
    let root = harness
        .runtime
        .block_on(seed_turn(&harness.pool, &fixture, "completed", 4, true))
        .expect("seeded root message");
    let stored = ItemId::new();
    harness.runtime.block_on(seed_item(
        &harness.pool,
        &fixture,
        3,
        stored.as_uuid(),
        "correction",
        r#"{"content":"committed"}"#,
        false,
        Some(root),
    ));
    harness.runtime.block_on(seed_item(
        &harness.pool,
        &fixture,
        2,
        Uuid::new_v4(),
        "correction",
        r#"{"content":"successor"}"#,
        false,
        Some(stored.as_uuid()),
    ));
    let before = harness.runtime.block_on(snapshot(&harness.pool, &fixture));
    assert_eq!(
        harness.correct(command(
            &fixture,
            stored,
            ItemId::from_uuid(root),
            "committed",
        )),
        Err(CorrectionError::CorruptHistory),
        "an exact retry must reject its sole sequence-misordered successor"
    );
    assert_unchanged(
        &before,
        &harness.runtime.block_on(snapshot(&harness.pool, &fixture)),
    );
}

/// A restored terminal flag on either message-root kind must not let fresh
/// admission or an exact retry accept malformed durable ancestry (CA-03/CA-04).
fn terminal_message_roots_fail_closed(harness: &Harness) {
    let corrupt = CorruptFixture::create(harness);
    for kind in ["user_message", "agent_message_delta"] {
        let fixture = fresh_fixture("terminal-message-root");
        harness
            .runtime
            .block_on(seed_turn(&harness.pool, &fixture, "completed", 3, true));
        let chain = harness
            .runtime
            .block_on(seed_chain(&corrupt.pool, &fixture, 2, None));
        harness.runtime.block_on(async {
            sqlx::query(
                "UPDATE turn_items SET item_type = $3, is_terminal = TRUE \
                 WHERE tenant_id = $1 AND item_id = $2",
            )
            .bind(fixture.tenant.as_str())
            .bind(chain[0])
            .bind(kind)
            .execute(&corrupt.pool)
            .await
            .expect("seed a terminal message root in the isolated history");
        });
        let before = harness.runtime.block_on(snapshot(&corrupt.pool, &fixture));
        assert_eq!(
            harness.correct_on(
                &corrupt.pool,
                command(
                    &fixture,
                    ItemId::new(),
                    ItemId::from_uuid(chain[1]),
                    "successor",
                )
            ),
            Err(CorrectionError::CorruptHistory),
            "fresh admission must reject a terminal {kind} root"
        );
        assert_unchanged(
            &before,
            &harness.runtime.block_on(snapshot(&corrupt.pool, &fixture)),
        );
        assert_rejected(
            harness,
            &corrupt.pool,
            &fixture,
            &chain,
            CorrectionError::CorruptHistory,
        );
    }
    corrupt.teardown();
}

/// Each case retains an exact stored identity while corrupting one ancestor.
fn schema_permitted_ancestry(harness: &Harness) {
    let pool = &harness.pool;
    let usage = r#"{"input_tokens":1,"output_tokens":1,"total_tokens":2}"#;
    for (count, index, kind, payload, expected) in [
        (2, 0, "usage", usage, CorrectionError::InvalidPredecessor),
        (2, 0, "user_message", "{}", CorrectionError::CorruptHistory),
        (3, 0, "user_message", "{}", CorrectionError::CorruptHistory),
        (3, 1, "correction", "{}", CorrectionError::CorruptHistory),
    ] {
        let fixture = fresh_fixture("retry-ancestor");
        harness.runtime.block_on(seed_turn(
            pool,
            &fixture,
            "completed",
            i64::try_from(count).expect("small chain") + 1,
            false,
        ));
        let chain = harness
            .runtime
            .block_on(seed_chain(pool, &fixture, count, None));
        harness.runtime.block_on(async {
            sqlx::query(
                "UPDATE turn_items SET item_type = $3, payload = $4 \
                 WHERE tenant_id = $1 AND item_id = $2",
            )
            .bind(fixture.tenant.as_str())
            .bind(chain[index])
            .bind(kind)
            .bind(payload)
            .execute(pool)
            .await
            .expect("the production schema permits the invalid ancestor");
        });
        assert_rejected(harness, pool, &fixture, &chain, expected);
    }
}

/// Exercises deeper broken links, cycles, and both direct and interior branches.
fn corrupt_links(harness: &Harness) {
    let corrupt = CorruptFixture::create(harness);
    let pool = &corrupt.pool;
    for shape in ["broken", "cycle", "direct_branch", "interior_branch"] {
        let fixture = fresh_fixture(shape);
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 5, false));
        let chain = harness
            .runtime
            .block_on(seed_chain(pool, &fixture, 3, None));
        if shape.ends_with("branch") {
            let target = if shape == "direct_branch" {
                chain[1]
            } else {
                chain[0]
            };
            harness.runtime.block_on(seed_item(
                pool,
                &fixture,
                4,
                Uuid::new_v4(),
                "correction",
                r#"{"content":"branch"}"#,
                false,
                Some(target),
            ));
        } else {
            let target = if shape == "cycle" {
                chain[1]
            } else {
                Uuid::new_v4()
            };
            harness.runtime.block_on(async {
                sqlx::query(
                    "UPDATE turn_items SET item_type = 'correction', corrects_item_id = $3 \
                     WHERE tenant_id = $1 AND item_id = $2",
                )
                .bind(fixture.tenant.as_str())
                .bind(chain[0])
                .bind(target)
                .execute(pool)
                .await
                .expect("seed a constraint-free broken or cyclic chain");
            });
        }
        assert_rejected(
            harness,
            pool,
            &fixture,
            &chain,
            CorrectionError::CorruptHistory,
        );
    }
    corrupt.teardown();
}

/// Verifies the production retry verdict and durable-state preservation.
fn assert_rejected(
    harness: &Harness,
    pool: &PgPool,
    fixture: &Fixture,
    chain: &[Uuid],
    expected: CorrectionError,
) {
    let before = harness.runtime.block_on(snapshot(pool, fixture));
    assert_eq!(
        harness.correct_on(
            pool,
            command(
                fixture,
                ItemId::from_uuid(chain[chain.len() - 1]),
                ItemId::from_uuid(chain[chain.len() - 2]),
                "c",
            )
        ),
        Err(expected),
        "an exact retry must validate its entire predecessor chain"
    );
    assert_unchanged(&before, &harness.runtime.block_on(snapshot(pool, fixture)));
}
