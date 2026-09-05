// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! AC-2: every Turn state, ownership dimension, Item kind, corrupt ancestor
//! shape, and stored-identity case admits, rejects, or fails closed exactly
//! as declared, with zero mutation on every rejection (ADR-0004 CA-02,
//! CA-03, CA-04, CA-05, and CA-09).

use koduck_ai::application::CorrectionError;
use koduck_ai::domain::ItemId;
use uuid::Uuid;

use crate::harness::{
    self, Fixture, Harness, assert_unchanged, command, fresh_fixture, seed_item, seed_turn,
    snapshot,
};

/// The approval-status canonical payload used for unsupported-kind seeds.
const APPROVAL_PAYLOAD: &str = "{\"approval_id\":\"00000000-0000-0000-0000-000000000001\",\
   \"attempt_id\":\"00000000-0000-0000-0000-000000000002\",\"status\":\"requested\",\
   \"decision\":null,\"version\":1}";

pub(crate) fn run() {
    let harness = Harness::connect(6);
    let pool = harness.pool.clone();
    terminal_states_admit(&harness, &pool);
    nonterminal_states_reject(&harness, &pool);
    ownership_matrix(&harness, &pool);
    predecessor_kinds(&harness, &pool);
    corrupt_ancestors(&harness, &pool);
    invalid_next_sequence(&harness, &pool);
    stored_identities(&harness, &pool);
    foreground_boundary(&harness);
}

/// The four terminal statuses admit a valid fresh tip and preserve every
/// lifecycle field; raw replay returns the originals plus exactly one
/// correction.
fn terminal_states_admit(harness: &Harness, pool: &sqlx::PgPool) {
    for status in ["completed", "failed", "interrupted", "cancelled"] {
        let fixture = fresh_fixture("ac2-terminal");
        let input = harness
            .runtime
            .block_on(seed_turn(pool, &fixture, status, 2, true));
        let input = ItemId::from_uuid(input.expect("seeded input item"));
        let before = harness.runtime.block_on(snapshot(pool, &fixture));
        let admitted = harness
            .correct(command(&fixture, ItemId::new(), input, "corrected"))
            .expect("a terminal turn admits a valid correction");
        assert_eq!(
            admitted.sequence, 2,
            "the admitted item takes next_sequence"
        );
        let after = harness.runtime.block_on(snapshot(pool, &fixture));
        assert_eq!(after.status, status);
        assert_eq!(after.next_sequence, before.next_sequence + 1);
        assert_eq!(after.item_rows, before.item_rows + 1);
        assert_eq!(after.lease_generation, before.lease_generation);
        assert!(!after.lease_fenced);
        assert_eq!(after.terminal_rows, before.terminal_rows);
        let replayed = harness.replay(&fixture.tenant, fixture.turn);
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].item_id, input);
        assert_eq!(replayed[1].sequence, 2);
        assert!(matches!(
            replayed[1].payload,
            koduck_ai::domain::ItemPayload::Correction(_)
        ));
    }
}

/// The two nonterminal statuses reject a fresh identity with zero mutation.
fn nonterminal_states_reject(harness: &Harness, pool: &sqlx::PgPool) {
    for status in ["started", "recovery-pending"] {
        let fixture = fresh_fixture("ac2-nonterminal");
        let input = harness
            .runtime
            .block_on(seed_turn(pool, &fixture, status, 2, true));
        let input = ItemId::from_uuid(input.expect("seeded input item"));
        let before = harness.runtime.block_on(snapshot(pool, &fixture));
        assert_eq!(
            harness.correct(command(&fixture, ItemId::new(), input, "corrected")),
            Err(CorrectionError::TurnNotTerminal),
            "a {status} turn must reject a fresh correction"
        );
        let after = harness.runtime.block_on(snapshot(pool, &fixture));
        assert_unchanged(&before, &after);
    }
}

/// Missing and non-owned targets are an indistinguishable `NotFound` with zero
/// mutation on the owned Turn.
fn ownership_matrix(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-ownership");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let before = harness.runtime.block_on(snapshot(pool, &fixture));

    let wrong_subject = koduck_ai::domain::TrustContext::new(fixture.tenant.clone(), "subject-b")
        .expect("valid trust context");
    let wrong_subject = koduck_ai::application::CorrectionCommand::new(
        wrong_subject,
        fixture.thread,
        fixture.turn,
        ItemId::new(),
        input,
        "corrected",
    )
    .expect("valid command shape");
    assert_eq!(
        harness.correct(wrong_subject),
        Err(CorrectionError::NotFound),
        "a non-owned subject must be indistinguishable from a missing target"
    );

    let drift = |thread: koduck_ai::domain::ThreadId, turn: koduck_ai::domain::TurnId| {
        command(
            &Fixture {
                tenant: fixture.tenant.clone(),
                subject: fixture.subject,
                thread,
                turn,
            },
            ItemId::new(),
            input,
            "corrected",
        )
    };
    assert_eq!(
        harness.correct(drift(koduck_ai::domain::ThreadId::new(), fixture.turn)),
        Err(CorrectionError::NotFound)
    );
    assert_eq!(
        harness.correct(drift(fixture.thread, koduck_ai::domain::TurnId::new())),
        Err(CorrectionError::NotFound)
    );
    let other_tenant = Fixture {
        tenant: koduck_ai::domain::TenantId::new(format!("cand11-wrong-tenant-{}", Uuid::new_v4()))
            .expect("valid tenant"),
        subject: fixture.subject,
        thread: fixture.thread,
        turn: fixture.turn,
    };
    assert_eq!(
        harness.correct(command(&other_tenant, ItemId::new(), input, "corrected")),
        Err(CorrectionError::NotFound)
    );

    let missing_predecessor = command(&fixture, ItemId::new(), ItemId::new(), "corrected");
    assert_eq!(
        harness.correct(missing_predecessor),
        Err(CorrectionError::InvalidPredecessor),
        "a missing predecessor is invalid"
    );

    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}

/// Every durable Item kind is classified exactly: message roots admit,
/// projection and terminal kinds are unsupported, and a valid correction
/// chain admits at its correction tip.
fn predecessor_kinds(harness: &Harness, pool: &sqlx::PgPool) {
    supported_message_roots_admit(harness, pool);
    unsupported_kinds_reject(harness, pool);
    correction_chain_tips_admit(harness, pool);
    unsupported_ancestry_end_rejects(harness, pool);
    stale_tip_conflicts(harness, pool);
}

/// The two message roots are admissible predecessors.
fn supported_message_roots_admit(harness: &Harness, pool: &sqlx::PgPool) {
    let supported = [
        ("user_message", r#"{"content":"root"}"#),
        ("agent_message_delta", r#"{"content":"delta"}"#),
    ];
    for (item_type, payload) in supported {
        let fixture = fresh_fixture("ac2-supported-root");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 2, false));
        let root = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool, &fixture, 1, root, item_type, payload, false, None,
        ));
        let root = ItemId::from_uuid(root);
        assert!(
            harness
                .correct(command(&fixture, ItemId::new(), root, "corrected"))
                .is_ok(),
            "a {item_type} root is an admissible predecessor"
        );
    }
}

/// Projection and terminal kinds are unsupported predecessors.
fn unsupported_kinds_reject(harness: &Harness, pool: &sqlx::PgPool) {
    let unsupported = [
        (
            "usage",
            "{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}",
        ),
        (
            "tool_call",
            "{\"descriptor_id\":\"d\",\"descriptor_version\":\"v1\",\
          \"target\":\"t\",\"attempt_id\":null,\"status\":null,\"version\":null}",
        ),
        (
            "tool_result",
            "{\"attempt_id\":null,\"status\":\"failed\",\
          \"code\":\"x\",\"effect_state\":null,\"output_bytes\":0,\
          \"output_digest\":null,\"version\":null}",
        ),
        ("approval_status", APPROVAL_PAYLOAD),
        (
            "completed",
            "{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}",
        ),
    ];
    for (item_type, payload) in unsupported {
        let fixture = fresh_fixture("ac2-unsupported");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 2, false));
        let terminal = item_type == "completed";
        let predecessor = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            1,
            predecessor,
            item_type,
            payload,
            terminal,
            None,
        ));
        assert_eq!(
            harness.correct(command(
                &fixture,
                ItemId::new(),
                ItemId::from_uuid(predecessor),
                "corrected",
            )),
            Err(CorrectionError::InvalidPredecessor),
            "a {item_type} predecessor must be unsupported"
        );
    }
}

/// A correction tip over a valid message root admits, over both roots.
fn correction_chain_tips_admit(harness: &Harness, pool: &sqlx::PgPool) {
    for root_type in ["user_message", "agent_message_delta"] {
        let fixture = fresh_fixture("ac2-correction-chain");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 3, false));
        let root = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            1,
            root,
            root_type,
            "{\"content\":\"root\"}",
            false,
            None,
        ));
        let tip = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            2,
            tip,
            "correction",
            "{\"content\":\"first\"}",
            false,
            Some(root),
        ));
        assert!(
            harness
                .correct(command(
                    &fixture,
                    ItemId::new(),
                    ItemId::from_uuid(tip),
                    "second",
                ))
                .is_ok(),
            "a correction tip over a {root_type} root admits"
        );
    }
}

/// A correction chain that terminates at an unsupported kind is invalid.
fn unsupported_ancestry_end_rejects(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-unsupported-ancestry");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 3, false));
    let usage = Uuid::new_v4();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        1,
        usage,
        "usage",
        "{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}",
        false,
        None,
    ));
    let tip = Uuid::new_v4();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        tip,
        "correction",
        "{\"content\":\"c\"}",
        false,
        Some(usage),
    ));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(tip),
            "corrected",
        )),
        Err(CorrectionError::InvalidPredecessor),
        "an ancestry that terminates at a usage item is unsupported"
    );
}
/// An otherwise valid tip with an existing direct successor conflicts.
fn stale_tip_conflicts(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-stale-tip");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 3, false));
    let root = Uuid::new_v4();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        1,
        root,
        "user_message",
        "{\"content\":\"root\"}",
        false,
        None,
    ));
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        Uuid::new_v4(),
        "correction",
        "{\"content\":\"successor\"}",
        false,
        Some(root),
    ));
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(root),
            "racing",
        )),
        Err(CorrectionError::PredecessorConflict),
        "a tip with an existing successor must conflict"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}

/// Corrupt ancestor shapes fail closed as `CorruptHistory`; shapes the
/// production constraints prevent are additionally proven rejected by the
/// unmodified production schema.
fn corrupt_ancestors(harness: &Harness, pool: &sqlx::PgPool) {
    cyclic_ancestry_fails_closed(harness, pool);
    forward_linked_ancestry_fails_closed(harness, pool);
    malformed_ancestor_payloads_fail_closed(harness, pool);
    constraint_prevented_shapes_fail_closed(harness, pool);
}

/// Cycle: C corrects A, A corrects B, B corrects C, seeded by later
/// updates because the foreign key forbids circular inserts.
fn cyclic_ancestry_fails_closed(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-cycle");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 4, false));
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for (offset, item_id) in ids.iter().enumerate() {
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            i64::try_from(offset).expect("cycle fits i64") + 1,
            *item_id,
            "user_message",
            "{\"content\":\"x\"}",
            false,
            None,
        ));
    }
    harness.runtime.block_on(async {
        for (sequence, target) in [(3usize, 1usize), (1, 2), (2, 0)] {
            sqlx::query(
                "UPDATE turn_items SET item_type = 'correction', \
                 payload = '{\"content\":\"c\"}', corrects_item_id = $5 \
                 WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
                 AND sequence = $4",
            )
            .bind(fixture.tenant.as_str())
            .bind(fixture.thread.as_uuid())
            .bind(fixture.turn.as_uuid())
            .bind(i64::try_from(sequence).expect("cycle fits i64"))
            .bind(ids[target])
            .execute(pool)
            .await
            .expect("seed a cycle link");
        }
    });
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(ids[2]),
            "corrected",
        )),
        Err(CorrectionError::CorruptHistory),
        "a cyclic ancestry must fail closed"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}
/// Nondecreasing order: a correction row pointing forward in sequence.
fn forward_linked_ancestry_fails_closed(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-nondecreasing");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 4, false));
    let later = Uuid::new_v4();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        4,
        later,
        "user_message",
        "{\"content\":\"later\"}",
        false,
        None,
    ));
    let tip = Uuid::new_v4();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        tip,
        "correction",
        "{\"content\":\"c\"}",
        false,
        Some(later),
    ));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(tip),
            "corrected",
        )),
        Err(CorrectionError::CorruptHistory),
        "a forward-linked ancestor must fail closed"
    );
}
/// Below-cap malformed ancestor payloads fail closed (CA-03), both as the
/// direct predecessor and deeper in the chain beneath a valid predecessor.
fn malformed_ancestor_payloads_fail_closed(harness: &Harness, pool: &sqlx::PgPool) {
    // A malformed payload deeper in the chain, beneath a valid direct
    // predecessor, must also fail closed (CA-03): the production schema
    // does not constrain payload JSON, so this case is seeded directly.
    {
        let fixture = fresh_fixture("ac2-malformed-deep");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 4, false));
        let root = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            1,
            root,
            "user_message",
            "{\"content\":\"root\"}",
            false,
            None,
        ));
        let older = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            2,
            older,
            "correction",
            "not json at all",
            false,
            Some(root),
        ));
        let tip = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            3,
            tip,
            "correction",
            "{\"content\":\"valid predecessor\"}",
            false,
            Some(older),
        ));
        let before = harness.runtime.block_on(snapshot(pool, &fixture));
        assert_eq!(
            harness.correct(command(
                &fixture,
                ItemId::new(),
                ItemId::from_uuid(tip),
                "corrected",
            )),
            Err(CorrectionError::CorruptHistory),
            "a malformed payload anywhere in the ancestry must fail closed"
        );
        let after = harness.runtime.block_on(snapshot(pool, &fixture));
        assert_unchanged(&before, &after);
    }

    for payload in ["not json at all", "{\"text\":\"no content member\"}"] {
        let fixture = fresh_fixture("ac2-malformed-ancestor");
        harness
            .runtime
            .block_on(seed_turn(pool, &fixture, "completed", 3, false));
        let root = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            1,
            root,
            "user_message",
            "{\"content\":\"root\"}",
            false,
            None,
        ));
        let tip = Uuid::new_v4();
        harness.runtime.block_on(seed_item(
            pool,
            &fixture,
            2,
            tip,
            "correction",
            payload,
            false,
            Some(root),
        ));
        assert_eq!(
            harness.correct(command(
                &fixture,
                ItemId::new(),
                ItemId::from_uuid(tip),
                "corrected",
            )),
            Err(CorrectionError::CorruptHistory),
            "a malformed ancestor payload must fail closed"
        );
    }
}
/// Broken ancestor links and interior branches are prevented by the
/// production constraints; the isolated fixture schema without those
/// constraints proves the admission walk rejects both, and the
/// unmodified production schema is separately proven to reject the rows.
fn constraint_prevented_shapes_fail_closed(harness: &Harness, pool: &sqlx::PgPool) {
    broken_ancestor_link_fails_closed(harness, pool);
    interior_branch_fails_closed(harness, pool);
}

/// The broken ancestor link only exists inside the constraint-free fixture
/// schema; the admission walk fails closed on it, and the unmodified
/// production foreign key is separately proven to reject the same row.
fn broken_ancestor_link_fails_closed(harness: &Harness, pool: &sqlx::PgPool) {
    let corrupt = harness::CorruptFixture::create(harness);
    let fixture = fresh_fixture("ac2-broken-link");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 3, false));
    let missing_target = Uuid::new_v4();
    let tip = Uuid::new_v4();
    harness.runtime.block_on(corrupt.seed_item(
        &fixture,
        1,
        tip,
        "correction",
        "{\"content\":\"c\"}",
        Some(missing_target),
    ));
    assert_eq!(
        harness.correct_on(
            &corrupt.pool,
            command(&fixture, ItemId::new(), ItemId::from_uuid(tip), "corrected",)
        ),
        Err(CorrectionError::CorruptHistory),
        "a broken ancestor link must fail closed"
    );
    corrupt.teardown();

    // The unmodified production constraints reject the same corrupt rows.
    harness.runtime.block_on(async {
        let result = sqlx::query(
            "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
             item_type, payload, is_terminal, corrects_item_id) \
             VALUES ($1, $2, $3, 99, $4, 'correction', '{\"content\":\"c\"}', FALSE, $5)",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .bind(Uuid::new_v4())
        .bind(missing_target)
        .execute(pool)
        .await;
        assert!(
            result.is_err(),
            "the production foreign key must reject a broken correction link"
        );
    });
}

/// Branched predecessors — tip and interior — only exist inside the
/// constraint-free fixture schema; both fail closed as corrupt durable
/// state (CA-03).
fn interior_branch_fails_closed(harness: &Harness, pool: &sqlx::PgPool) {
    let corrupt = harness::CorruptFixture::create(harness);
    let seed = |fixture: &Fixture,
                sequence: i64,
                item_id: Uuid,
                item_type: &str,
                payload: &str,
                corrects| {
        harness
            .runtime
            .block_on(corrupt.seed_item(fixture, sequence, item_id, item_type, payload, corrects));
    };
    let branched_fixture = fresh_fixture("ac2-branch");
    harness
        .runtime
        .block_on(seed_turn(pool, &branched_fixture, "completed", 4, false));
    let root = Uuid::new_v4();
    seed(
        &branched_fixture,
        1,
        root,
        "user_message",
        "{\"content\":\"r\"}",
        None,
    );
    seed(
        &branched_fixture,
        2,
        Uuid::new_v4(),
        "correction",
        "{\"content\":\"a\"}",
        Some(root),
    );
    seed(
        &branched_fixture,
        3,
        Uuid::new_v4(),
        "correction",
        "{\"content\":\"b\"}",
        Some(root),
    );
    assert_eq!(
        harness.correct_on(
            &corrupt.pool,
            command(
                &branched_fixture,
                ItemId::new(),
                ItemId::from_uuid(root),
                "racing",
            )
        ),
        Err(CorrectionError::CorruptHistory),
        "a branched predecessor is corrupt durable state, not a stale tip"
    );
    let mid = Uuid::new_v4();
    seed(
        &branched_fixture,
        4,
        mid,
        "correction",
        "{\"content\":\"m\"}",
        Some(root),
    );
    seed(
        &branched_fixture,
        5,
        Uuid::new_v4(),
        "correction",
        "{\"content\":\"n\"}",
        Some(mid),
    );
    seed(
        &branched_fixture,
        6,
        Uuid::new_v4(),
        "correction",
        "{\"content\":\"o\"}",
        Some(mid),
    );
    assert_eq!(
        harness.correct_on(
            &corrupt.pool,
            command(
                &branched_fixture,
                ItemId::new(),
                ItemId::from_uuid(mid),
                "corrected",
            )
        ),
        Err(CorrectionError::CorruptHistory),
        "an interior branch must fail closed"
    );
    corrupt.teardown();
}

/// Sequence-counter corruption fails closed; the nonpositive shape is
/// proven impossible under the unmodified production schema.
fn invalid_next_sequence(harness: &Harness, pool: &sqlx::PgPool) {
    stale_and_overflow_counters_fail_closed(harness, pool);
    nonpositive_counter_schema_proof(harness, pool);
}

/// Stale counter: an existing sequence at or above `next_sequence`; and the
/// `BIGINT` ceiling: an incrementable counter at `i64::MAX` cannot advance.
fn stale_and_overflow_counters_fail_closed(harness: &Harness, pool: &sqlx::PgPool) {
    // Stale counter: an existing sequence at or above next_sequence.
    let fixture = fresh_fixture("ac2-stale-counter");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        5,
        Uuid::new_v4(),
        "agent_message_delta",
        "{\"content\":\"ahead of the counter\"}",
        false,
        None,
    ));
    let input = harness.runtime.block_on(async {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT item_id FROM turn_items WHERE tenant_id = $1 AND sequence = 1 \
             AND thread_id = $2 AND turn_id = $3",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the seeded input identity")
    });
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(input),
            "x"
        )),
        Err(CorrectionError::CorruptHistory),
        "a stale next_sequence must fail closed"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);

    // BIGINT overflow: an incrementable counter at i64::MAX cannot advance.
    let fixture = fresh_fixture("ac2-overflow");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", i64::MAX, true));
    let input = harness.runtime.block_on(async {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT item_id FROM turn_items WHERE tenant_id = $1 AND sequence = 1 \
             AND thread_id = $2 AND turn_id = $3",
        )
        .bind(fixture.tenant.as_str())
        .bind(fixture.thread.as_uuid())
        .bind(fixture.turn.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the seeded input identity")
    });
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_eq!(
        harness.correct(command(
            &fixture,
            ItemId::new(),
            ItemId::from_uuid(input),
            "x"
        )),
        Err(CorrectionError::CorruptHistory),
        "an overflowing next_sequence must fail closed"
    );
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);
}
/// The production check constraint proves a nonpositive counter can never
/// exist.
fn nonpositive_counter_schema_proof(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-overflow-schema");
    harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    harness.runtime.block_on(async {
        let result = sqlx::query("UPDATE turns SET next_sequence = 0 WHERE tenant_id = $1")
            .bind(fixture.tenant.as_str())
            .execute(pool)
            .await;
        assert!(
            result.is_err(),
            "the production check must reject a nonpositive next_sequence"
        );
    });
}

/// Stored caller-stable identities resolve exactly: exact retries return the
/// original durable Item with zero writes, every drift is an
/// `IdentityConflict`, and malformed or oversized stored payloads fail
/// closed or bounded before content equality is evaluated.
fn stored_identities(harness: &Harness, pool: &sqlx::PgPool) {
    exact_retry_returns_original(harness, pool);
    identity_drift_conflicts(harness, pool);
    nonterminal_and_malformed_stored_identities(harness, pool);
}

/// An exact retry returns the original durable Item with zero writes, even
/// after a later successor exists.
fn exact_retry_returns_original(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-retry");
    // The fixture mirrors the lawful post-admission state: the correction at
    // sequence 2 exists, so next_sequence has advanced to 3 (later to 4).
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 4, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        2,
        identity.as_uuid(),
        "correction",
        "{\"content\":\"committed\"}",
        false,
        Some(input.as_uuid()),
    ));
    let before = harness.runtime.block_on(snapshot(pool, &fixture));
    let retried = harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("an exact retry returns the original item");
    assert_eq!(retried.item_id, identity);
    assert_eq!(retried.sequence, 2);
    let after = harness.runtime.block_on(snapshot(pool, &fixture));
    assert_unchanged(&before, &after);

    // The exact retry still returns the original after a later successor.
    let later = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &fixture,
        3,
        later.as_uuid(),
        "correction",
        "{\"content\":\"later\"}",
        false,
        Some(identity.as_uuid()),
    ));
    let retried_again = harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("the exact retry survives a later successor");
    assert_eq!(retried_again.item_id, identity);
    assert_eq!(retried_again.sequence, 2);
}

/// Every identity-bound field drift is an `IdentityConflict`.
fn identity_drift_conflicts(harness: &Harness, pool: &sqlx::PgPool) {
    let fixture = fresh_fixture("ac2-drift");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness
        .correct(command(&fixture, identity, input, "committed"))
        .expect("the original commits");
    let drift_content = command(&fixture, identity, input, "different");
    assert_eq!(
        harness.correct(drift_content),
        Err(CorrectionError::IdentityConflict)
    );
    let drift_predecessor = command(&fixture, identity, ItemId::new(), "committed");
    assert_eq!(
        harness.correct(drift_predecessor),
        Err(CorrectionError::IdentityConflict)
    );
    let other_turn = Fixture {
        tenant: fixture.tenant.clone(),
        subject: fixture.subject,
        thread: fixture.thread,
        turn: koduck_ai::domain::TurnId::new(),
    };
    harness
        .runtime
        .block_on(seed_turn(pool, &other_turn, "completed", 2, true));
    let drift_turn = command(&other_turn, identity, input, "committed");
    assert_eq!(
        harness.correct(drift_turn),
        Err(CorrectionError::IdentityConflict),
        "a stored identity bound to another turn is an identity conflict"
    );

    // A non-correction row under the caller identity is an identity conflict.
    let kind_fixture = fresh_fixture("ac2-kind-drift");
    harness
        .runtime
        .block_on(seed_turn(pool, &kind_fixture, "completed", 2, false));
    let kind_identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &kind_fixture,
        1,
        kind_identity.as_uuid(),
        "user_message",
        "{\"content\":\"not a correction\"}",
        false,
        None,
    ));
    assert_eq!(
        harness.correct(command(&kind_fixture, kind_identity, ItemId::new(), "x")),
        Err(CorrectionError::IdentityConflict),
        "a stored non-correction identity is an identity conflict"
    );
}
/// An exact match on a nonterminal turn is inconsistent durable state, and
/// malformed below-cap stored retry payloads fail closed before content
/// equality can be evaluated.
fn nonterminal_and_malformed_stored_identities(harness: &Harness, pool: &sqlx::PgPool) {
    // An exact match on a nonterminal turn is inconsistent durable state.
    let nonterminal = fresh_fixture("ac2-nonterminal-retry");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &nonterminal, "started", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &nonterminal,
        2,
        identity.as_uuid(),
        "correction",
        "{\"content\":\"committed\"}",
        false,
        Some(input.as_uuid()),
    ));
    assert_eq!(
        harness.correct(command(&nonterminal, identity, input, "committed")),
        Err(CorrectionError::CorruptHistory),
        "an exact stored match on a nonterminal turn is corruption"
    );
    // Identity drift still takes precedence on the same nonterminal turn.
    assert_eq!(
        harness.correct(command(&nonterminal, identity, input, "drift")),
        Err(CorrectionError::IdentityConflict)
    );

    // A stored exact match whose sequence reached the Turn counter is
    // corrupt durable state, not a resolvable retry (CA-03/CA-05).
    let stale_fixture = fresh_fixture("ac2-retry-stale-sequence");
    let input = harness
        .runtime
        .block_on(seed_turn(pool, &stale_fixture, "completed", 2, true));
    let input = ItemId::from_uuid(input.expect("seeded input item"));
    let identity = ItemId::new();
    harness.runtime.block_on(seed_item(
        pool,
        &stale_fixture,
        5,
        identity.as_uuid(),
        "correction",
        "{\"content\":\"beyond the counter\"}",
        false,
        Some(input.as_uuid()),
    ));
    let before = harness.runtime.block_on(snapshot(pool, &stale_fixture));
    assert_eq!(
        harness.correct(command(
            &stale_fixture,
            identity,
            input,
            "beyond the counter",
        )),
        Err(CorrectionError::CorruptHistory),
        "a retry at or above next_sequence must fail closed"
    );
    let after = harness.runtime.block_on(snapshot(pool, &stale_fixture));
    assert_unchanged(&before, &after);

    // Malformed below-cap stored retry payloads fail closed before content
    // equality can be evaluated.
    for payload in [
        "{\"content\":",
        "{\"text\":\"no content\"}",
        "{\"content\":1}",
    ] {
        let corrupt_fixture = fresh_fixture("ac2-malformed-retry");
        let input =
            harness
                .runtime
                .block_on(seed_turn(pool, &corrupt_fixture, "completed", 2, true));
        let input = ItemId::from_uuid(input.expect("seeded input item"));
        let identity = ItemId::new();
        harness.runtime.block_on(seed_item(
            pool,
            &corrupt_fixture,
            2,
            identity.as_uuid(),
            "correction",
            payload,
            false,
            Some(input.as_uuid()),
        ));
        assert_eq!(
            harness.correct(command(&corrupt_fixture, identity, input, "committed")),
            Err(CorrectionError::CorruptHistory),
            "a malformed stored retry payload must fail closed"
        );
    }
}

/// The CA-09 boundary: ordinary foreground append keeps rejecting the
/// terminal Turn, including after a correction.
fn foreground_boundary(harness: &Harness) {
    let (fixture, accepted) = harness::foreground_turn_to_terminal(harness, "ac2-foreground");
    assert_eq!(
        harness::foreground_append_rejected(harness, &accepted),
        koduck_ai::application::HistoryError::AlreadyTerminal,
    );
    let admitted = harness
        .correct(command(
            &fixture,
            ItemId::new(),
            accepted.input.item_id,
            "corrected after terminal",
        ))
        .expect("the terminal turn admits the correction");
    assert_eq!(admitted.sequence, 3);
    assert_eq!(
        harness::foreground_append_rejected(harness, &accepted),
        koduck_ai::application::HistoryError::AlreadyTerminal,
        "foreground append must still reject after a correction"
    );
}
