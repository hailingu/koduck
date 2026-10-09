// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! CAND-13 focused tests for the typed provider-context preparation seam
//! (ADR-0006 PC-02 through PC-04): complete per-Turn groups with explicit
//! source provenance are validated atomically, projected through the
//! unchanged CAND-12 owner, and converted into one owned effective provider
//! view. Additional fixtures drive the runner, the production `PostgreSQL`
//! read, the OpenAI-compatible serializer through a real `Reqwest` request,
//! and the HTTP problem boundary; the full AC-1 through AC-9 matrix remains
//! the T-2 deliverable.

use super::fixtures::*;
use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest};
use koduck_ai::adapters::provider::{OpenAiProtocolTransport, ReqwestOpenAiTransport};
use koduck_ai::application::{
    AcceptedTurn, CommittedToolCall, CorrectionCommand, CorrectionStore, HistoryError, ModelInput,
    ModelProvider, ModelToolCall, ModelToolResult, NewItem, PriorTurnHistory, PriorTurnRow,
    ProjectionError, ProjectionScope, ProviderContextError, ProviderError, ProviderEvent,
    ProviderHistoryItem, ProviderHistoryKind, ToolRound, TurnCommand, TurnHistory, TurnRunner,
    prepare_provider_history,
};
use koduck_ai::domain::item_correction::ItemCorrection;
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

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

/// Serializes disposable-database migration across this target's parallel
/// tests: the production migration list is not concurrency-safe when two
/// fixtures race to apply it on one fresh database.
static DATABASE_SETUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let setup_guard = DATABASE_SETUP_LOCK
        .lock()
        .expect("setup lock is unpoisoned");
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
    drop(setup_guard);
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

/// In-memory canonical history double for runner-level fixtures.
#[derive(Default, Clone)]
struct MemoryHistory {
    state: Rc<RefCell<MemoryState>>,
}

#[derive(Default)]
struct MemoryState {
    turn_order: Vec<(ThreadId, TurnId)>,
    turn_items: BTreeMap<TurnId, Vec<Item>>,
    accepted: usize,
}

impl MemoryHistory {
    /// Appends one correction Item directly, standing in for CAND-11
    /// admission after a terminal.
    fn inject_correction(&self, turn_id: TurnId, target: ItemId, content: &str) -> Item {
        let mut state = self.state.borrow_mut();
        let items = state.turn_items.get_mut(&turn_id).expect("seeded Turn");
        let correction = Item::new(
            items.len() as u64 + 1,
            ItemPayload::Correction(
                ItemCorrection::new(content, target).expect("valid correction content"),
            ),
        );
        items.push(correction.clone());
        correction
    }

    /// Seeds a Turn whose stored rows already violate the raw replay or
    /// ancestry contract, standing in for corrupt canonical history.
    fn seed_corrupt_turn(&self, thread_id: ThreadId, turn_id: TurnId, rows: Vec<Item>) {
        let mut state = self.state.borrow_mut();
        state.turn_order.push((thread_id, turn_id));
        state.turn_items.insert(turn_id, rows);
    }

    fn accepted_count(&self) -> usize {
        self.state.borrow().accepted
    }
}

impl TurnHistory for MemoryHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        Err(HistoryError::NotFound)
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_turns(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        let state = self.state.borrow();
        let mut groups = Vec::new();
        for (owned_thread, turn_id) in &state.turn_order {
            if *owned_thread != thread_id {
                continue;
            }
            let scope = ProjectionScope::new(
                trust.tenant_id.clone(),
                &trust.subject_id,
                thread_id,
                *turn_id,
            )
            .expect("valid double scope");
            let rows = state
                .turn_items
                .get(turn_id)
                .map(|items| {
                    items
                        .iter()
                        .cloned()
                        .map(|item| PriorTurnRow::new(item, scope.clone()))
                        .collect()
                })
                .unwrap_or_default();
            groups.push(PriorTurnHistory::new(*turn_id, rows));
        }
        Ok(groups)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let thread_id = command.thread_id.unwrap_or_default();
        let turn_id = TurnId::new();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        let mut state = self.state.borrow_mut();
        state.turn_order.push((thread_id, turn_id));
        state.turn_items.insert(turn_id, vec![input.clone()]);
        state.accepted += 1;
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            thread_id,
            turn_id,
            koduck_ai::domain::LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.borrow_mut();
        let items = state
            .turn_items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.state
            .borrow()
            .turn_items
            .get(&turn_id)
            .cloned()
            .ok_or(HistoryError::NotFound)
    }
}

/// Provider double capturing every `ModelInput` and completing each stream.
struct RecordingProvider {
    inputs: Rc<RefCell<Vec<ModelInput>>>,
}

impl ModelProvider for RecordingProvider {
    fn stream(
        &mut self,
        input: ModelInput,
    ) -> Result<koduck_ai::application::ProviderStream<'_>, ProviderError> {
        self.inputs.borrow_mut().push(input);
        Ok(Box::new(
            vec![
                ProviderEvent::Delta("A".to_owned()),
                ProviderEvent::Usage(Usage::new(1, 1).expect("valid usage")),
                ProviderEvent::Completed,
            ]
            .into_iter(),
        ))
    }
}

/// PC-02/PC-04 through the runner: a resumed Turn prepares the effective
/// view before acceptance, sends it as `ModelInput.history`, and leaves the
/// canonical raw rows unchanged.
#[test]
fn runner_resumes_from_the_prepared_effective_history() {
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(
        RecordingProvider {
            inputs: Rc::clone(&inputs),
        },
        history.clone(),
    );
    let first = runner
        .execute(TurnCommand::new(command_trust(), None, "draft").expect("valid command"))
        .expect("first Turn completes");
    let immutable_first = first.replay.clone();
    let correction =
        history.inject_correction(first.turn_id, immutable_first[0].item_id, "revised");

    let second = runner
        .execute(
            TurnCommand::new(command_trust(), Some(first.thread_id), "second")
                .expect("valid resumed command"),
        )
        .expect("resumed Turn completes");

    assert_eq!(second.thread_id, first.thread_id);
    assert_eq!(
        history
            .replay(&command_trust().tenant_id, first.turn_id)
            .expect("first replay remains readable"),
        {
            let mut expected = immutable_first.clone();
            expected.push(correction.clone());
            expected
        },
        "canonical raw history keeps every original row plus the correction"
    );
    let recorded = inputs.borrow();
    assert!(
        recorded[0].history.is_empty(),
        "a new Thread carries no prior context"
    );
    assert_eq!(
        recorded[1]
            .history
            .iter()
            .map(|view| (view.item_id, view.kind, view.content.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                immutable_first[0].item_id,
                ProviderHistoryKind::UserMessage,
                Some("revised".to_owned())
            ),
            (
                immutable_first[1].item_id,
                ProviderHistoryKind::AgentMessageDelta,
                Some("A".to_owned())
            ),
            (immutable_first[2].item_id, ProviderHistoryKind::Usage, None),
            (
                immutable_first[3].item_id,
                ProviderHistoryKind::Terminal,
                None
            ),
        ],
        "the resumed request carries the effective view once, with no correction entry"
    );
}

/// PC-07 at the HTTP boundary: a corrupt correction chain rejects resume as
/// the existing 503 problem before any acceptance, stream, or provider call.
#[test]
fn context_rejection_maps_to_the_503_problem_before_any_stream() {
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let history = MemoryHistory::default();
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    // A correction strictly precedes its target: the raw structure pass
    // accepts the reference, so ancestry reports the forward reference.
    let later_root = user_item(2, "later root");
    history.seed_corrupt_turn(
        thread_id,
        turn_id,
        vec![
            correction_item(1, later_root.item_id, "early correction"),
            later_root,
        ],
    );
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        RecordingProvider {
            inputs: Rc::clone(&inputs),
        },
        history.clone(),
    ));
    let request = HttpRequest {
        method: HttpMethod::Post,
        path: "/api/v1/ai/chat".to_owned(),
        content_type: Some("application/json".to_owned()),
        body: format!(
            r#"{{"input":"hello","thread_id":"{}"}}"#,
            thread_id.as_uuid()
        ),
        trust: Some(command_trust()),
    };

    let response = adapter.handle(request);

    assert_eq!(response.status, 503);
    assert_eq!(
        response.header("Content-Type"),
        Some("application/problem+json")
    );
    assert!(
        response
            .body
            .contains("\"code\":\"durability-unavailable\"")
    );
    assert_eq!(history.accepted_count(), 0, "no Turn is accepted");
    assert!(inputs.borrow().is_empty(), "no provider request is made");
}

/// Reads exactly one HTTP request (headers plus `Content-Length` body) on a
/// loopback connection and replies with the scripted raw response bytes.
fn sse_upstream(response: &'static [u8]) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback upstream");
    let address = listener.local_addr().expect("loopback address");
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept provider request");
        let mut captured: Vec<u8> = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).expect("read provider request");
            captured.extend_from_slice(&chunk[..read]);
            if request_complete(&captured) {
                break;
            }
        }
        sender
            .send(String::from_utf8(captured).expect("request is UTF-8"))
            .expect("request receiver lives");
        stream.write_all(response).expect("write scripted response");
    });
    (format!("http://{address}"), receiver)
}

/// Reports whether the captured bytes hold a complete HTTP/1.1 request.
fn request_complete(received: &[u8]) -> bool {
    let Some(headers_end) = received.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&received[..headers_end]);
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    received.len() >= headers_end + 4 + length
}

/// Builds the prepared-view input whose wire translation is asserted.
fn corrected_transport_input() -> ModelInput {
    let user = ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 1,
        kind: ProviderHistoryKind::UserMessage,
        source_item_id: ItemId::new(),
        content: Some("revised".to_owned()),
    };
    let delta = ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 2,
        kind: ProviderHistoryKind::AgentMessageDelta,
        source_item_id: ItemId::new(),
        content: Some("answer".to_owned()),
    };
    let usage = ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 3,
        kind: ProviderHistoryKind::Usage,
        source_item_id: ItemId::new(),
        content: None,
    };
    let terminal = ProviderHistoryItem {
        item_id: ItemId::new(),
        sequence: 4,
        kind: ProviderHistoryKind::Terminal,
        source_item_id: ItemId::new(),
        content: None,
    };
    ModelInput {
        tenant_id: TenantId::new(TENANT).expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        input: "next".to_owned(),
        history: vec![user, delta, usage, terminal],
        tool_rounds: vec![ToolRound {
            assistant_content: String::new(),
            calls: vec![CommittedToolCall {
                call: ModelToolCall {
                    name: "fixture.tool".to_owned(),
                    arguments: "{}".to_owned(),
                },
                result: ModelToolResult {
                    content: "ok".to_owned(),
                    is_error: false,
                },
            }],
        }],
    }
}

/// PC-05 through the production transport: the real `Reqwest` request body
/// carries the effective messages — corrected root at its original position,
/// no correction entry, current input once, then committed Tool rounds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn effective_messages_reach_the_production_transport() {
    let scripted: &'static [u8] =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
          data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n\n\
          data: [DONE]\n\n";
    let (base_url, request_receiver) = sse_upstream(scripted);
    let mut transport = ReqwestOpenAiTransport::new(
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client"),
        tokio::runtime::Handle::current(),
        &base_url,
        "test-model",
        "test-key",
    );

    let frames = transport
        .chat_completion_frames(&corrected_transport_input())
        .expect("provider stream opens");
    for frame in frames {
        assert!(frame.is_ok(), "scripted frames decode: {frame:?}");
    }

    let request = request_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("upstream captured the request");
    let body = request
        .split_once("\r\n\r\n")
        .expect("captured request has a body")
        .1;
    let document: serde_json::Value = serde_json::from_str(body).expect("request body is JSON");
    assert_eq!(
        document["messages"],
        serde_json::json!([
            { "role": "user", "content": "revised" },
            { "role": "assistant", "content": "answer" },
            { "role": "user", "content": "next" },
            {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "id": "call_0",
                        "type": "function",
                        "function": { "name": "fixture.tool", "arguments": "{}" }
                    }
                ]
            },
            { "role": "tool", "tool_call_id": "call_0", "content": "ok" }
        ]),
        "the wire carries the effective view exactly once with current input and Tool rounds"
    );
}

/// PC-01 empty/foreign outcomes through the production forwarding path: an
/// owned Thread with no rows returns empty groups, and a different subject
/// receives the indistinguishable `NotFound` rejection.
#[test]
fn owned_empty_thread_and_foreign_subject_read_outcomes() {
    let Some((pool, runtime)) = migrated_pool() else {
        return;
    };
    let owner = TrustContext::new(
        TenantId::new(format!("ci-{}", uuid::Uuid::new_v4())).expect("unique tenant"),
        "owner",
    )
    .expect("owner trust context");
    let thread_id = ThreadId::new();
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)",
            )
            .bind(owner.tenant_id.as_str())
            .bind("owner")
            .bind(thread_id.as_uuid())
            .execute(&pool),
        )
        .expect("seed an owned empty Thread");
    let history = PostgresTurnHistory::new(SqlxPostgresExecutor::new(
        pool.clone(),
        runtime.handle().clone(),
    ));

    let groups = TurnHistory::prior_thread_turns(&history, &owner, thread_id)
        .expect("an owned empty Thread returns empty history");
    assert!(groups.is_empty());

    let intruder =
        TrustContext::new(owner.tenant_id.clone(), "intruder").expect("intruder trust context");
    assert_eq!(
        TurnHistory::prior_thread_turns(&history, &intruder, thread_id),
        Err(HistoryError::NotFound),
        "a different subject observes no thread"
    );

    runtime.block_on(pool.close());
}
