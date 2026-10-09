// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-1: the production `SQLx` read preserves actual per-Turn provenance and
//! canonical order — equal timestamps tie-broken by Turn UUID, local
//! sequences restarting per Turn, sequence gaps, a nonterminal prior Turn,
//! and post-terminal corrections — without modifying source history, and
//! independent barrier-started reads agree.

use std::sync::{Arc, Barrier};

use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::application::{
    PriorTurnHistory, ProjectionScope, ProviderHistoryKind, TurnHistory, prepare_provider_history,
};
use koduck_ai::domain::{
    Item, ItemId, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};

use super::support::*;
use crate::fixtures::*;

/// One seeded Turn of the ordering fixture.
struct FixtureTurn {
    turn_id: TurnId,
    items: Vec<Item>,
}

/// The complete ordering fixture of one owned Thread.
struct OrderingFixture {
    turns: Vec<FixtureTurn>,
    corrected_root: ItemId,
}

/// Runs the complete AC-1 check against isolated migrated `PostgreSQL`.
pub(crate) fn run() {
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let thread_id = ThreadId::new();
    let fixture = seed_ordering_fixture(&runtime, &pool, &tenant, &subject, thread_id);
    let before = snapshot_fixture(&runtime, &pool, &tenant, thread_id, &fixture);

    let groups = executor
        .prior_thread_turns(&owner, thread_id)
        .expect("the production read returns the Thread's ordered groups");

    assert_group_order_and_provenance(&groups, &fixture, &tenant, &subject, thread_id);
    assert_effective_order(&owner, thread_id, &groups, &fixture);
    assert_independent_reads_agree(&executor, &owner, thread_id, &groups);
    assert_owned_empty_thread_returns_no_groups(&runtime, &pool, &tenant, &subject);
    assert_snapshots_unchanged(&runtime, &pool, &tenant, thread_id, &fixture, &before);

    runtime.block_on(pool.close());
}

/// Seeds one Thread whose canonical tuple order differs from pure Turn UUID
/// order: two Turns share a timestamp (tie broken by UUID) and the newest
/// Turn carries the smallest UUID.
fn seed_ordering_fixture(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    subject: &str,
    thread_id: ThreadId,
) -> OrderingFixture {
    seed_thread_row(runtime, pool, &owner_trust(tenant, subject), thread_id);
    let early = TurnId::from_uuid(uuid::Uuid::from_u128(2));
    let tied = TurnId::from_uuid(uuid::Uuid::from_u128(3));
    let late_small_uuid = TurnId::from_uuid(uuid::Uuid::from_u128(1));
    for (turn, created_at, status) in [
        (early, "2026-01-01T00:00:00+00:00", "completed"),
        (tied, "2026-01-01T00:00:00+00:00", "started"),
        (late_small_uuid, "2026-01-03T00:00:00+00:00", "completed"),
    ] {
        seed_turn_row(runtime, pool, tenant, thread_id, turn, created_at, status);
    }

    // Terminal Turn with a sequence gap and one post-terminal correction of
    // its user root; local sequences restart at 1 in every Turn.
    let root = user_item(1, "one");
    let terminal = terminal_item(
        5,
        TerminalOutcome::Completed {
            usage: Usage::new(3, 5).expect("valid usage"),
        },
    );
    let correction = correction_item(6, root.item_id, "one-revised");
    let corrected_root = match &correction.payload {
        koduck_ai::domain::ItemPayload::Correction(link) => link.corrects_item_id(),
        _ => unreachable!("the correction item was just built"),
    };
    let early_items = vec![root, delta_item(2, "answer-one"), terminal, correction];
    // A nonterminal prior Turn: no terminal row exists.
    let tied_items = vec![user_item(1, "two"), usage_item(2)];
    let late_items = vec![
        user_item(1, "three"),
        delta_item(2, "answer-three"),
        terminal_item(
            3,
            TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            },
        ),
    ];
    seed_item_rows(runtime, pool, tenant, thread_id, early, &early_items);
    seed_item_rows(runtime, pool, tenant, thread_id, tied, &tied_items);
    seed_item_rows(
        runtime,
        pool,
        tenant,
        thread_id,
        late_small_uuid,
        &late_items,
    );

    OrderingFixture {
        turns: vec![
            FixtureTurn {
                turn_id: early,
                items: early_items,
            },
            FixtureTurn {
                turn_id: tied,
                items: tied_items,
            },
            FixtureTurn {
                turn_id: late_small_uuid,
                items: late_items,
            },
        ],
        corrected_root,
    }
}

/// Captures the canonical rows of every fixture Turn before any read.
fn snapshot_fixture(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
    fixture: &OrderingFixture,
) -> Vec<Vec<Item>> {
    fixture
        .turns
        .iter()
        .map(|turn| sql_items_of(runtime, pool, tenant, thread_id, turn.turn_id))
        .collect()
}

/// Asserts the returned groups follow the canonical tuple order with exact
/// per-row scope and complete per-Turn membership.
fn assert_group_order_and_provenance(
    groups: &[PriorTurnHistory],
    fixture: &OrderingFixture,
    tenant: &TenantId,
    subject: &str,
    thread_id: ThreadId,
) {
    assert_eq!(
        groups
            .iter()
            .map(|group| group.source_turn)
            .collect::<Vec<_>>(),
        fixture
            .turns
            .iter()
            .map(|turn| turn.turn_id)
            .collect::<Vec<_>>(),
        "groups follow created_at, then Turn UUID, with the newest Turn last despite its smallest UUID"
    );
    for (group, turn) in groups.iter().zip(&fixture.turns) {
        assert!(!group.rows.is_empty(), "every returned group is complete");
        let expected = ProjectionScope::new(tenant.clone(), subject, thread_id, turn.turn_id)
            .expect("valid expected scope");
        assert_eq!(
            group
                .rows
                .iter()
                .map(|row| row.item.item_id)
                .collect::<Vec<_>>(),
            turn.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            "each group owns its complete Turn, including post-terminal corrections"
        );
        for row in &group.rows {
            assert_eq!(row.scope, expected, "every row reports its actual scope");
        }
    }
    let early_sequences: Vec<u64> = groups[0].rows.iter().map(|row| row.item.sequence).collect();
    assert_eq!(
        early_sequences,
        vec![1, 2, 5, 6],
        "the sequence gap survives"
    );
    assert!(
        groups[1].rows.iter().all(|row| !matches!(
            row.item.payload,
            koduck_ai::domain::ItemPayload::Terminal(_)
        )),
        "the nonterminal prior Turn carries no terminal row"
    );
}

/// Asserts preparation projects the groups into the ordered effective view.
fn assert_effective_order(
    owner: &TrustContext,
    thread_id: ThreadId,
    groups: &[PriorTurnHistory],
    fixture: &OrderingFixture,
) {
    let prepared =
        prepare_provider_history(owner, thread_id, groups).expect("the ordered fixture prepares");
    assert_eq!(
        prepared
            .iter()
            .map(|view| (view.kind, view.effective_text().map(str::to_owned)))
            .collect::<Vec<_>>(),
        vec![
            (
                ProviderHistoryKind::UserMessage,
                Some("one-revised".to_owned())
            ),
            (
                ProviderHistoryKind::AgentMessageDelta,
                Some("answer-one".to_owned())
            ),
            (ProviderHistoryKind::Terminal, None),
            (ProviderHistoryKind::UserMessage, Some("two".to_owned())),
            (ProviderHistoryKind::Usage, None),
            (ProviderHistoryKind::UserMessage, Some("three".to_owned())),
            (
                ProviderHistoryKind::AgentMessageDelta,
                Some("answer-three".to_owned())
            ),
            (ProviderHistoryKind::Terminal, None),
        ],
        "the effective view follows canonical group order with the post-terminal correction applied"
    );
    assert_eq!(prepared[0].item_id, fixture.corrected_root);
    assert_ne!(
        prepared[0].source_item_id, prepared[0].item_id,
        "the corrected root selects the correction as its effective source"
    );
}

/// Asserts two independent barrier-started reads return identical groups.
fn assert_independent_reads_agree(
    executor: &SqlxPostgresExecutor,
    owner: &TrustContext,
    thread_id: ThreadId,
    groups: &[PriorTurnHistory],
) {
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let executor = executor.clone();
        let owner = owner.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            executor
                .prior_thread_turns(&owner, thread_id)
                .expect("the independent read succeeds")
        }));
    }
    for handle in handles {
        let independent = handle.join().expect("the reader thread joins");
        assert_eq!(
            independent
                .iter()
                .map(independent_group_identity)
                .collect::<Vec<_>>(),
            groups
                .iter()
                .map(independent_group_identity)
                .collect::<Vec<_>>(),
            "independent reads agree on group order and membership"
        );
    }
}

/// Reduces one group to its comparable identity: source Turn plus row identities.
fn independent_group_identity(group: &PriorTurnHistory) -> (TurnId, Vec<ItemId>) {
    (
        group.source_turn,
        group.rows.iter().map(|row| row.item.item_id).collect(),
    )
}

/// Asserts an owned Thread with no rows returns an empty group collection.
fn assert_owned_empty_thread_returns_no_groups(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    subject: &str,
) {
    let owner = owner_trust(tenant, subject);
    let empty_thread = ThreadId::new();
    seed_thread_row(runtime, pool, &owner, empty_thread);
    let history = koduck_ai::adapters::history::postgres::PostgresTurnHistory::new(
        SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone()),
    );
    let groups = TurnHistory::prior_thread_turns(&history, &owner, empty_thread)
        .expect("an owned empty Thread returns empty history");
    assert!(groups.is_empty());
    assert_eq!(
        TurnHistory::prior_thread_turns(&history, &owner, ThreadId::new(),),
        Err(koduck_ai::application::HistoryError::NotFound),
        "an unknown Thread is not owned"
    );
}

/// Asserts every canonical snapshot is unchanged by the reads, the
/// preparations, and the independent barrier-started readers.
fn assert_snapshots_unchanged(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    thread_id: ThreadId,
    fixture: &OrderingFixture,
    before: &[Vec<Item>],
) {
    let after = snapshot_fixture(runtime, pool, tenant, thread_id, fixture);
    assert_eq!(
        before, after,
        "reads and preparations leave source history unchanged"
    );
}
