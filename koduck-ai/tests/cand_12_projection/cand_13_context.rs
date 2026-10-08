// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! CAND-13 focused tests for the typed provider-context preparation seam
//! (ADR-0006 PC-02 through PC-04): complete per-Turn groups with explicit
//! source provenance are validated atomically, projected through the
//! unchanged CAND-12 owner, and converted into one owned effective provider
//! view. Database, transport, and HTTP acceptance checks are delivered with
//! the T-2 suite; these fixtures exercise the pure typed seam only.

use super::fixtures::*;
use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::application::{
    AcceptedTurn, CorrectionCommand, CorrectionStore, NewItem, PriorTurnHistory, PriorTurnRow,
    ProjectionError, ProjectionScope, ProviderContextError, ProviderHistoryItem,
    ProviderHistoryKind, TurnCommand, prepare_provider_history,
};
use koduck_ai::domain::{
    Item, ItemId, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};

const TENANT: &str = "tenant-a";
const SUBJECT: &str = "subject-a";

fn command_trust() -> TrustContext {
    TrustContext::new(TenantId::new(TENANT).expect("valid tenant"), SUBJECT)
        .expect("valid command trust")
}

fn owned_scope(thread: ThreadId, turn: TurnId) -> ProjectionScope {
    ProjectionScope::new(
        TenantId::new(TENANT).expect("valid tenant"),
        SUBJECT,
        thread,
        turn,
    )
    .expect("valid row scope")
}

/// Wraps one canonical Item as a row reporting the given owned scope.
fn owned_row(thread: ThreadId, turn: TurnId, item: Item) -> PriorTurnRow {
    PriorTurnRow::new(item, owned_scope(thread, turn))
}

fn completed_usage() -> Usage {
    Usage::new(1, 1).expect("valid usage")
}

/// PC-02/PC-04: one complete source Turn prepares the ordered effective view;
/// a correction after a terminal replaces its earlier root at the original
/// position and creates no separate entry.
#[test]
fn corrected_source_turn_prepares_the_effective_provider_view() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let draft = user_item(1, "draft");
    let answer = delta_item(2, "answer");
    let terminal = terminal_item(
        3,
        TerminalOutcome::Completed {
            usage: completed_usage(),
        },
    );
    let correction = correction_item(4, draft.item_id, "revised");

    let prepared = prepare_provider_history(
        &trust,
        thread,
        &[PriorTurnHistory::new(
            turn,
            vec![
                owned_row(thread, turn, draft.clone()),
                owned_row(thread, turn, answer.clone()),
                owned_row(thread, turn, terminal),
                owned_row(thread, turn, correction.clone()),
            ],
        )],
    )
    .expect("a complete valid source Turn prepares one owned view");

    assert_eq!(prepared.len(), 3, "the correction creates no separate view");
    assert_eq!(prepared[0].item_id, draft.item_id);
    assert_eq!(prepared[0].sequence, 1);
    assert_eq!(prepared[0].kind, ProviderHistoryKind::UserMessage);
    assert_eq!(prepared[0].content.as_deref(), Some("revised"));
    assert_eq!(prepared[0].source_item_id, correction.item_id);
    assert_eq!(prepared[1].item_id, answer.item_id);
    assert_eq!(prepared[1].kind, ProviderHistoryKind::AgentMessageDelta);
    assert_eq!(prepared[1].content.as_deref(), Some("answer"));
    assert_eq!(prepared[1].source_item_id, answer.item_id);
    assert_eq!(prepared[2].kind, ProviderHistoryKind::Terminal);
    assert_eq!(prepared[2].content, None);
    assert_eq!(prepared[2].source_item_id, prepared[2].item_id);
}

/// PC-02: an empty group collection remains valid empty history, while any
/// empty group — alone or after a valid prefix — rejects as
/// `InvalidProvenance` before projection.
#[test]
fn empty_group_collection_is_valid_but_any_empty_group_rejects() {
    let trust = command_trust();
    let thread = ThreadId::new();

    let prepared = prepare_provider_history(&trust, thread, &[])
        .expect("an empty group collection is valid empty history");
    assert!(prepared.is_empty());

    assert_eq!(
        prepare_provider_history(
            &trust,
            thread,
            &[PriorTurnHistory::new(TurnId::new(), Vec::new())]
        ),
        Err(ProviderContextError::InvalidProvenance),
        "an empty group alone rejects before projection"
    );

    let valid_turn = TurnId::new();
    let valid = PriorTurnHistory::new(
        valid_turn,
        vec![owned_row(thread, valid_turn, user_item(1, "kept"))],
    );
    let empty = PriorTurnHistory::new(TurnId::new(), Vec::new());
    assert_eq!(
        prepare_provider_history(&trust, thread, &[valid, empty]),
        Err(ProviderContextError::InvalidProvenance),
        "an empty group after a valid prefix still rejects the whole context"
    );
}

/// PC-02: one source Turn split across two groups — adjacent or interleaved
/// by another Turn — is a duplicate group identity and rejects as
/// `InvalidProvenance` before projection.
#[test]
fn split_or_interleaved_source_turn_rejects_as_duplicate_group_identity() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let first = TurnId::new();
    let second = TurnId::new();

    let split = vec![
        PriorTurnHistory::new(
            first,
            vec![owned_row(thread, first, user_item(1, "half one"))],
        ),
        PriorTurnHistory::new(
            first,
            vec![owned_row(thread, first, delta_item(2, "half two"))],
        ),
    ];
    assert_eq!(
        prepare_provider_history(&trust, thread, &split),
        Err(ProviderContextError::InvalidProvenance),
        "an adjacent split of one Turn rejects"
    );

    let interleaved = vec![
        PriorTurnHistory::new(
            first,
            vec![owned_row(thread, first, user_item(1, "first start"))],
        ),
        PriorTurnHistory::new(
            second,
            vec![owned_row(thread, second, user_item(1, "second whole"))],
        ),
        PriorTurnHistory::new(
            first,
            vec![owned_row(thread, first, delta_item(2, "first rest"))],
        ),
    ];
    assert_eq!(
        prepare_provider_history(&trust, thread, &interleaved),
        Err(ProviderContextError::InvalidProvenance),
        "an interleaved split of one Turn rejects"
    );
}

/// PC-02: CAND-12 alone validates row scope; a foreign row at index 0 or a
/// later position returns the exact group-local `ScopeMismatch` index, and the
/// expected Turn comes from the group's explicit `source_turn` rather than its
/// first row.
#[test]
fn row_scope_mismatch_retains_the_exact_projection_cause_and_index() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let group_turn = TurnId::new();
    let row_turn = TurnId::new();

    let foreign_first = PriorTurnHistory::new(
        group_turn,
        vec![
            owned_row(thread, row_turn, user_item(1, "foreign")),
            owned_row(thread, group_turn, delta_item(2, "owned")),
        ],
    );
    assert_eq!(
        prepare_provider_history(&trust, thread, &[foreign_first]),
        Err(ProviderContextError::Projection(
            ProjectionError::ScopeMismatch { index: 0 }
        )),
        "a row/group Turn mismatch rejects even at row index 0"
    );

    let foreign_later = PriorTurnHistory::new(
        group_turn,
        vec![
            owned_row(thread, group_turn, user_item(1, "owned")),
            owned_row(thread, group_turn, delta_item(2, "owned")),
            owned_row(thread, row_turn, usage_item(3)),
        ],
    );
    assert_eq!(
        prepare_provider_history(&trust, thread, &[foreign_later]),
        Err(ProviderContextError::Projection(
            ProjectionError::ScopeMismatch { index: 2 }
        )),
        "a later foreign row reports its first group-local index"
    );
}

/// PC-02/PC-04: independent groups prepare in returned order, an uncorrected
/// Turn keeps its own source identity, and inert kinds retain their
/// payload-kind semantics without content.
#[test]
fn independent_groups_prepare_in_order_with_unchanged_views() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let first_turn = TurnId::new();
    let second_turn = TurnId::new();
    let first_item = user_item(1, "first");
    let usage = usage_item(2);
    let call = tool_call_item(3);
    let result = tool_result_item(4);
    let approval = approval_item(5);
    let second_item = delta_item(1, "second");

    let prepared = prepare_provider_history(
        &trust,
        thread,
        &[
            PriorTurnHistory::new(
                first_turn,
                vec![
                    owned_row(thread, first_turn, first_item.clone()),
                    owned_row(thread, first_turn, usage),
                    owned_row(thread, first_turn, call),
                    owned_row(thread, first_turn, result),
                    owned_row(thread, first_turn, approval),
                ],
            ),
            PriorTurnHistory::new(
                second_turn,
                vec![owned_row(thread, second_turn, second_item.clone())],
            ),
        ],
    )
    .expect("independent complete Turns prepare in order");

    assert_eq!(prepared.len(), 6);
    assert_eq!(prepared[0].item_id, first_item.item_id);
    assert_eq!(prepared[0].content.as_deref(), Some("first"));
    assert_eq!(prepared[0].source_item_id, first_item.item_id);
    assert_eq!(prepared[1].kind, ProviderHistoryKind::Usage);
    assert_eq!(prepared[2].kind, ProviderHistoryKind::ToolCall);
    assert_eq!(prepared[3].kind, ProviderHistoryKind::ToolResult);
    assert_eq!(prepared[4].kind, ProviderHistoryKind::ApprovalStatus);
    assert!(prepared[1..=4].iter().all(|view| view.content.is_none()));
    assert_eq!(prepared[5].item_id, second_item.item_id);
    assert_eq!(prepared[5].content.as_deref(), Some("second"));
}

/// PC-02/PC-07: a later malformed group rejects the whole context with its
/// exact CAND-12 cause; no partial prefix is returned.
#[test]
fn later_invalid_group_rejects_the_whole_prepared_context() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let valid_turn = TurnId::new();
    let invalid_turn = TurnId::new();
    let root = user_item(1, "root");
    // The target exists in the Turn but sits strictly after the correction,
    // so the raw structure pass accepts and ancestry reports the forward
    // reference (EP-02 runs before EP-03).
    let later_root = user_item(2, "later root");

    let prepared = prepare_provider_history(
        &trust,
        thread,
        &[
            PriorTurnHistory::new(valid_turn, vec![owned_row(thread, valid_turn, root)]),
            PriorTurnHistory::new(
                invalid_turn,
                vec![
                    owned_row(
                        thread,
                        invalid_turn,
                        correction_item(1, later_root.item_id, "early correction"),
                    ),
                    owned_row(thread, invalid_turn, later_root),
                ],
            ),
        ],
    );

    assert_eq!(
        prepared,
        Err(ProviderContextError::Projection(
            ProjectionError::ForwardReference
        )),
        "the valid first group produces no partial prefix"
    );
}

/// Connects an isolated migrated `PostgreSQL` when the test database is
/// configured; `None` skips the real-`SQLx` smoke.
fn migrated_pool() -> Option<(sqlx::postgres::PgPool, tokio::runtime::Runtime)> {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").ok()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("PostgreSQL smoke runtime");
    let pool = runtime
        .block_on(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(4)
                .connect(&database_url),
        )
        .expect("connect to disposable PostgreSQL");
    for migration in [
        include_str!("../../migrations/0001_cand_1_history.sql"),
        include_str!("../../migrations/0002_cand_2_policy_execution.sql"),
        include_str!("../../migrations/0003_cand_2_requester_ownership.sql"),
        include_str!("../../migrations/0004_cand_2_tool_projections.sql"),
        include_str!("../../migrations/0005_cand_2_execution_attempts.sql"),
        include_str!("../../migrations/0006_cand_2_interrupt_barrier.sql"),
        include_str!("../../migrations/0007_cand_2_tool_audit.sql"),
        include_str!("../../migrations/0008_cand_2_interruption_approval_cancellation.sql"),
        include_str!("../../migrations/0009_cand_3_correction_items.sql"),
    ] {
        runtime
            .block_on(async { sqlx::raw_sql(migration).execute(&pool).await })
            .expect("apply production migration");
    }
    Some((pool, runtime))
}

/// One seeded two-Turn Thread: a terminal first Turn with a post-terminal
/// correction of its user root, followed by a resumed second Turn.
struct SeededThread {
    first: AcceptedTurn,
    second: AcceptedTurn,
    corrected_item_id: ItemId,
}

/// Seeds the two-Turn Thread fixture through the production executor.
fn seed_two_turn_thread(executor: &SqlxPostgresExecutor) -> SeededThread {
    let owner = TrustContext::new(
        TenantId::new(format!("ci-{}", uuid::Uuid::new_v4())).expect("unique tenant"),
        "owner",
    )
    .expect("owner trust context");
    let first = executor
        .accept_initial(&TurnCommand::new(owner.clone(), None, "draft").expect("valid command"))
        .expect("accept first Turn");
    executor
        .append(
            &first,
            NewItem::AgentMessageDelta {
                content: "answer".to_owned(),
            },
        )
        .expect("append assistant delta");
    executor
        .append(
            &first,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            }),
        )
        .expect("append terminal");
    let correction = executor
        .correct(
            CorrectionCommand::new(
                owner.clone(),
                first.thread_id,
                first.turn_id,
                ItemId::new(),
                first.input.item_id,
                "revised",
            )
            .expect("valid correction command"),
        )
        .expect("admit post-terminal correction");
    let second = executor
        .accept_initial(
            &TurnCommand::new(owner.clone(), Some(first.thread_id), "second")
                .expect("valid resumed command"),
        )
        .expect("accept second Turn");
    SeededThread {
        first,
        second,
        corrected_item_id: correction.item_id,
    }
}

/// Asserts the complete provenance of every returned group against replay.
fn assert_grouped_provenance(
    executor: &SqlxPostgresExecutor,
    seeded: &SeededThread,
    groups: &[PriorTurnHistory],
    tenant: &TenantId,
) {
    assert_eq!(groups.len(), 2, "one group per source Turn");
    assert_eq!(groups[0].source_turn, seeded.first.turn_id);
    assert_eq!(groups[1].source_turn, seeded.second.turn_id);
    let replay = executor
        .replay(tenant, seeded.first.turn_id)
        .expect("first Turn replay");
    assert_eq!(
        groups[0]
            .rows
            .iter()
            .map(|row| row.item.item_id)
            .collect::<Vec<_>>(),
        replay.iter().map(|item| item.item_id).collect::<Vec<_>>(),
        "the complete first Turn — including the post-terminal correction — matches replay order"
    );
    for group in groups {
        assert!(!group.rows.is_empty());
        let expected = ProjectionScope::new(
            tenant.clone(),
            "owner",
            seeded.first.thread_id,
            group.source_turn,
        )
        .expect("valid expected scope");
        for row in &group.rows {
            assert_eq!(row.scope, expected);
        }
    }
}

/// PC-01/PC-02 through real `SQLx`: the bounded read returns complete
/// Turn-scoped groups with actual per-row provenance — including a
/// correction committed after a terminal — and preparation turns them into
/// the ordered effective view. The full order/limit/deadline matrix is the
/// T-2 AC-1/AC-5 suite; this smoke proves the replaced seam end to end.
#[test]
fn production_read_groups_real_turns_for_preparation() {
    let Some((pool, runtime)) = migrated_pool() else {
        return;
    };
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let seeded = seed_two_turn_thread(&executor);
    let owner =
        TrustContext::new(seeded.first.tenant_id.clone(), "owner").expect("owner trust context");

    let groups = executor
        .prior_thread_turns(&owner, seeded.first.thread_id)
        .expect("production read returns the Thread's Turn groups");
    assert_grouped_provenance(&executor, &seeded, &groups, &owner.tenant_id);

    let prepared: Vec<ProviderHistoryItem> =
        prepare_provider_history(&owner, seeded.first.thread_id, &groups)
            .expect("real grouped history prepares");
    assert_eq!(
        prepared
            .iter()
            .map(|view| (
                view.kind,
                view.content.as_deref(),
                view.source_item_id == seeded.corrected_item_id,
            ))
            .collect::<Vec<_>>(),
        vec![
            (ProviderHistoryKind::UserMessage, Some("revised"), true),
            (
                ProviderHistoryKind::AgentMessageDelta,
                Some("answer"),
                false
            ),
            (ProviderHistoryKind::Terminal, None, false),
            (ProviderHistoryKind::UserMessage, Some("second"), false),
        ],
        "the corrected root carries the replacement and no separate correction view exists"
    );

    runtime.block_on(pool.close());
}
