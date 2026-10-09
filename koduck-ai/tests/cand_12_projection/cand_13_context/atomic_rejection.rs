// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-3: invalid source anywhere rejects the whole prepared context before
//! acceptance or dispatch, without contaminating later independent requests.
//! Every EP-06 category retains its exact typed cause; only empty groups or
//! duplicate `source_turn` values reject as `InvalidProvenance`; a real
//! corrupt `SQL` payload fails as `Unavailable`; the unchanged corrupt Thread
//! keeps rejecting while a separate valid Thread succeeds on the same runner.

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest};
use koduck_ai::application::{
    HistoryError, PriorTurnHistory, PriorTurnRow, ProjectionError, ProjectionScope,
    ProviderContextError, TurnCommand, TurnRunError, TurnRunner, prepare_provider_history,
};
use koduck_ai::domain::item_correction::RawReplayStructureError;
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};

use super::support::*;
use crate::fixtures::*;

/// Runs the complete AC-3 check.
pub(crate) fn run() {
    pure_rejection_matrix();
    runner_rejects_atomically_and_recovers_independently();
    real_sql_corruption_persists_and_neighbor_thread_succeeds();
}

/// One invalid-group case with its exact expected projection cause.
struct ProjectionCase {
    label: &'static str,
    error: ProjectionError,
    group: PriorTurnHistory,
}

/// Every scope-mismatch category as one invalid group behind a valid prefix.
fn scope_mismatch_cases(thread: ThreadId) -> Vec<ProjectionCase> {
    let invalid_turn = TurnId::new();
    let row = |item: Item, scope: ProjectionScope| PriorTurnRow::new(item, scope);
    let local = |item: Item| owned_row(thread, invalid_turn, item);
    let drifted = |tenant: &str, subject: &str, thread: ThreadId, turn: TurnId| {
        ProjectionScope::new(
            TenantId::new(tenant).expect("valid tenant"),
            subject,
            thread,
            turn,
        )
        .expect("valid drifted scope")
    };
    let delta = delta_item(2, "kept");
    let usage = usage_item(3);
    vec![
        ProjectionCase {
            label: "tenant drift at row 0",
            error: ProjectionError::ScopeMismatch { index: 0 },
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![row(
                    user_item(1, "foreign"),
                    drifted("tenant-b", "subject-a", thread, invalid_turn),
                )],
            ),
        },
        ProjectionCase {
            label: "subject drift at row 0",
            error: ProjectionError::ScopeMismatch { index: 0 },
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![row(
                    user_item(1, "foreign"),
                    drifted("tenant-a", "subject-b", thread, invalid_turn),
                )],
            ),
        },
        ProjectionCase {
            label: "thread drift at row 0",
            error: ProjectionError::ScopeMismatch { index: 0 },
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![row(
                    user_item(1, "foreign"),
                    drifted("tenant-a", "subject-a", ThreadId::new(), invalid_turn),
                )],
            ),
        },
        ProjectionCase {
            label: "row/group Turn mismatch at row 0",
            error: ProjectionError::ScopeMismatch { index: 0 },
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![row(
                    user_item(1, "foreign"),
                    drifted("tenant-a", "subject-a", thread, TurnId::new()),
                )],
            ),
        },
        ProjectionCase {
            label: "row/group Turn mismatch at a later index",
            error: ProjectionError::ScopeMismatch { index: 2 },
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![
                    local(user_item(1, "owned")),
                    local(delta),
                    row(
                        usage,
                        drifted("tenant-a", "subject-a", thread, TurnId::new()),
                    ),
                ],
            ),
        },
    ]
}

/// Every raw-structure and ancestry category as one invalid group.
fn structure_cases(thread: ThreadId) -> Vec<ProjectionCase> {
    let invalid_turn = TurnId::new();
    let local = |item: Item| owned_row(thread, invalid_turn, item);
    let user = user_item(1, "root");
    let unsupported_root = usage_item(1);
    let late_target = delta_item(3, "late");
    vec![
        ProjectionCase {
            label: "non-increasing sequence",
            error: ProjectionError::InvalidReplay(RawReplayStructureError::NonIncreasingSequence),
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![local(user_item(1, "first")), local(user_item(1, "repeat"))],
            ),
        },
        ProjectionCase {
            label: "duplicate item identity",
            error: ProjectionError::InvalidReplay(RawReplayStructureError::DuplicateItemIdentity),
            group: duplicate_identity_group(thread, invalid_turn),
        },
        ProjectionCase {
            label: "unknown correction target",
            error: ProjectionError::InvalidReplay(RawReplayStructureError::UnknownCorrectionTarget),
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![
                    local(user_item(1, "root")),
                    local(correction_item(2, ItemId::new(), "orphan")),
                ],
            ),
        },
        ProjectionCase {
            label: "branched correction chain",
            error: ProjectionError::InvalidReplay(RawReplayStructureError::DuplicateSuccessor),
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![
                    local(user.clone()),
                    local(correction_item(2, user.item_id, "one")),
                    local(correction_item(3, user.item_id, "two")),
                ],
            ),
        },
        ProjectionCase {
            label: "self correction",
            error: ProjectionError::InvalidReplay(RawReplayStructureError::SelfCorrection),
            group: self_correction_group(thread, invalid_turn),
        },
        ProjectionCase {
            label: "forward reference",
            error: ProjectionError::ForwardReference,
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![
                    local(delta_item(1, "earlier")),
                    local(correction_item(2, late_target.item_id, "jumps ahead")),
                    local(late_target),
                ],
            ),
        },
        ProjectionCase {
            label: "correction cycle",
            error: ProjectionError::ForwardReference,
            group: cycle_group(thread, invalid_turn),
        },
        ProjectionCase {
            label: "unsupported root",
            error: ProjectionError::UnsupportedRoot,
            group: PriorTurnHistory::new(
                invalid_turn,
                vec![
                    local(unsupported_root.clone()),
                    local(correction_item(2, unsupported_root.item_id, "rewrite")),
                ],
            ),
        },
    ]
}

/// Builds one group whose two rows share an identity.
fn duplicate_identity_group(thread: ThreadId, turn: TurnId) -> PriorTurnHistory {
    let shared = ItemId::new();
    let items = vec![
        Item {
            item_id: shared,
            sequence: 1,
            payload: ItemPayload::UserMessage {
                content: "one".to_owned(),
            },
        },
        Item {
            item_id: shared,
            sequence: 2,
            payload: ItemPayload::UserMessage {
                content: "two".to_owned(),
            },
        },
    ];
    PriorTurnHistory::new(
        turn,
        items
            .into_iter()
            .map(|item| owned_row(thread, turn, item))
            .collect(),
    )
}

/// Builds one group holding a self-referential correction.
fn self_correction_group(thread: ThreadId, turn: TurnId) -> PriorTurnHistory {
    let self_id = ItemId::new();
    let item = Item {
        item_id: self_id,
        sequence: 1,
        payload: correction_payload(self_id, "self"),
    };
    PriorTurnHistory::new(turn, vec![owned_row(thread, turn, item)])
}

/// Builds one group holding a two-correction cycle.
fn cycle_group(thread: ThreadId, turn: TurnId) -> PriorTurnHistory {
    let one = ItemId::new();
    let two = ItemId::new();
    let items = vec![
        Item {
            item_id: one,
            sequence: 1,
            payload: correction_payload(two, "first"),
        },
        Item {
            item_id: two,
            sequence: 2,
            payload: correction_payload(one, "second"),
        },
    ];
    PriorTurnHistory::new(
        turn,
        items
            .into_iter()
            .map(|item| owned_row(thread, turn, item))
            .collect(),
    )
}

/// Pure preparation matrix: every projection category rejects the whole
/// context after a valid prefix, and group-structure faults reject as
/// `InvalidProvenance` before projection.
fn pure_rejection_matrix() {
    let trust = command_trust();
    let thread = ThreadId::new();
    let valid_turn = TurnId::new();
    let valid = PriorTurnHistory::new(
        valid_turn,
        vec![owned_row(thread, valid_turn, user_item(1, "kept"))],
    );
    let mut cases = scope_mismatch_cases(thread);
    cases.extend(structure_cases(thread));
    for case in cases {
        assert_eq!(
            prepare_provider_history(&trust, thread, &[valid.clone(), case.group]),
            Err(ProviderContextError::Projection(case.error)),
            "case '{}' rejects the whole context with its exact cause",
            case.label
        );
    }
    let invalid_turn = TurnId::new();
    let empty = PriorTurnHistory::new(invalid_turn, Vec::new());
    assert_eq!(
        prepare_provider_history(&trust, thread, std::slice::from_ref(&empty)),
        Err(ProviderContextError::InvalidProvenance),
        "an empty group alone rejects before projection"
    );
    assert_eq!(
        prepare_provider_history(&trust, thread, &[valid.clone(), empty]),
        Err(ProviderContextError::InvalidProvenance),
        "an empty group after a valid prefix rejects before projection"
    );
    let other_turn = TurnId::new();
    let split = vec![
        PriorTurnHistory::new(
            invalid_turn,
            vec![owned_row(thread, invalid_turn, user_item(1, "half"))],
        ),
        PriorTurnHistory::new(
            invalid_turn,
            vec![owned_row(thread, invalid_turn, delta_item(2, "other half"))],
        ),
        PriorTurnHistory::new(
            other_turn,
            vec![owned_row(thread, other_turn, user_item(1, "whole"))],
        ),
    ];
    assert_eq!(
        prepare_provider_history(&trust, thread, &split),
        Err(ProviderContextError::InvalidProvenance),
        "a duplicate source_turn — split or interleaved — rejects before projection"
    );
}

/// One runner-level rejection fixture and its exact expected error shape.
struct RunnerCase {
    label: &'static str,
    expected: ExpectedRejection,
    seed: Seed,
}

/// Seeds one runner-level rejection fixture for the given identities.
type Seed = Box<dyn FnOnce(&MemoryHistory, ThreadId, TurnId)>;

/// The exact typed rejection one runner case must produce.
enum ExpectedRejection {
    ScopeMismatch(usize),
    InvalidReplay(RawReplayStructureError),
    ForwardReference,
    UnsupportedRoot,
    InvalidProvenance,
    HistoryUnavailable,
}

/// Runner-level atomicity: every representative category rejects the resume
/// before acceptance or dispatch, the unchanged corrupt Thread keeps
/// rejecting on retry, and a separate valid Thread succeeds on the same runner.
fn runner_rejects_atomically_and_recovers_independently() {
    for case in runner_cases() {
        let provider = RecordingProvider::new();
        let history = MemoryHistory::default();
        let thread_id = ThreadId::new();
        let turn_id = TurnId::new();
        let RunnerCase {
            label,
            expected,
            seed,
        } = case;
        seed(&history, thread_id, turn_id);
        let mut observer_events = 0_usize;
        let mut runner = TurnRunner::new(provider.clone(), history.clone());
        let command = TurnCommand::new(command_trust(), Some(thread_id), "resume")
            .expect("valid resumed command");
        let result = runner.execute_with_observer(command, &mut |_| observer_events += 1);
        assert_rejection(
            label,
            &expected,
            &result,
            observer_events,
            &history,
            &provider,
        );
        let retry = runner.execute(
            TurnCommand::new(command_trust(), Some(thread_id), "retry")
                .expect("valid retry command"),
        );
        assert_rejection(label, &expected, &retry, 0, &history, &provider);
        // An outage case recovers only after the outage clears; corrupt-source
        // cases must not contaminate an independent valid Thread at all.
        history.clear_read_failures();
        assert_independent_valid_thread_succeeds(&mut runner, &provider, &history);
    }
}

/// Builds the representative runner-level rejection cases.
fn runner_cases() -> Vec<RunnerCase> {
    vec![
        RunnerCase {
            label: "row/group Turn mismatch at row 0",
            expected: ExpectedRejection::ScopeMismatch(0),
            seed: Box::new(|history, thread, turn| {
                history.seed_turn_with_row_scope(
                    thread,
                    turn,
                    vec![user_item(1, "foreign"), delta_item(2, "owned")],
                    TurnId::new(),
                );
            }),
        },
        RunnerCase {
            label: "raw structure fault",
            expected: ExpectedRejection::InvalidReplay(
                RawReplayStructureError::NonIncreasingSequence,
            ),
            seed: Box::new(|history, thread, turn| {
                history.seed_turn(
                    thread,
                    turn,
                    vec![user_item(1, "first"), user_item(1, "repeat")],
                );
            }),
        },
        RunnerCase {
            label: "forward reference",
            expected: ExpectedRejection::ForwardReference,
            seed: Box::new(|history, thread, turn| {
                let late = delta_item(3, "late");
                history.seed_turn(
                    thread,
                    turn,
                    vec![
                        delta_item(1, "earlier"),
                        correction_item(2, late.item_id, "early"),
                        late,
                    ],
                );
            }),
        },
        RunnerCase {
            label: "unsupported root",
            expected: ExpectedRejection::UnsupportedRoot,
            seed: Box::new(|history, thread, turn| {
                let usage = usage_item(1);
                history.seed_turn(
                    thread,
                    turn,
                    vec![usage.clone(), correction_item(2, usage.item_id, "rewrite")],
                );
            }),
        },
        RunnerCase {
            label: "empty group",
            expected: ExpectedRejection::InvalidProvenance,
            seed: Box::new(|history, thread, turn| {
                history.seed_turn(thread, turn, Vec::new());
            }),
        },
        RunnerCase {
            label: "pre-acceptance read failure",
            expected: ExpectedRejection::HistoryUnavailable,
            seed: Box::new(|history, _thread, _turn| {
                history.fail_all_prior_reads(HistoryError::Unavailable);
            }),
        },
    ]
}

/// Asserts one runner outcome is the exact typed rejection with zero effects.
fn assert_rejection(
    label: &str,
    expected: &ExpectedRejection,
    result: &Result<koduck_ai::application::TurnResult, TurnRunError>,
    observer_events: usize,
    history: &MemoryHistory,
    provider: &RecordingProvider,
) {
    let matched = match result {
        Err(TurnRunError::Context(ProviderContextError::Projection(cause))) => {
            match (&expected, &cause) {
                (
                    ExpectedRejection::ScopeMismatch(index),
                    ProjectionError::ScopeMismatch { index: seen },
                ) if *seen == *index => true,
                (ExpectedRejection::InvalidReplay(want), ProjectionError::InvalidReplay(seen))
                    if seen == want =>
                {
                    true
                }
                (ExpectedRejection::ForwardReference, ProjectionError::ForwardReference)
                | (ExpectedRejection::UnsupportedRoot, ProjectionError::UnsupportedRoot) => true,
                _ => false,
            }
        }
        Err(TurnRunError::Context(ProviderContextError::InvalidProvenance)) => {
            matches!(expected, ExpectedRejection::InvalidProvenance)
        }
        Err(TurnRunError::History(HistoryError::Unavailable)) => {
            matches!(expected, ExpectedRejection::HistoryUnavailable)
        }
        _ => false,
    };
    assert!(matched, "case '{label}' rejects with its exact typed cause");
    assert_eq!(
        history.accepted_count(),
        0,
        "case '{label}' accepts nothing"
    );
    assert!(
        provider.recorded().is_empty(),
        "case '{label}' dispatches nothing"
    );
    assert_eq!(observer_events, 0, "case '{label}' starts no stream event");
}

/// Asserts one independently valid Thread still succeeds on the same runner.
fn assert_independent_valid_thread_succeeds(
    runner: &mut TurnRunner<RecordingProvider, MemoryHistory>,
    provider: &RecordingProvider,
    history: &MemoryHistory,
) {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let root = user_item(1, "ok");
    let correction = correction_item(4, root.item_id, "fixed");
    history.seed_turn(
        thread_id,
        turn_id,
        vec![
            root,
            delta_item(2, "fine"),
            terminal_item(
                3,
                TerminalOutcome::Completed {
                    usage: Usage::new(1, 1).expect("valid usage"),
                },
            ),
            correction,
        ],
    );
    let accepted_before = history.accepted_count();
    runner
        .execute(
            TurnCommand::new(command_trust(), Some(thread_id), "next")
                .expect("valid independent command"),
        )
        .expect("the independent valid Thread succeeds on the same runner");
    assert_eq!(history.accepted_count(), accepted_before + 1);
    let recorded = provider.recorded();
    let input = recorded.last().expect("the valid Thread dispatched once");
    assert_eq!(
        input
            .history
            .iter()
            .map(|view| (view.kind, view.effective_text().map(str::to_owned)))
            .collect::<Vec<_>>(),
        vec![
            (
                koduck_ai::application::ProviderHistoryKind::UserMessage,
                Some("fixed".to_owned())
            ),
            (
                koduck_ai::application::ProviderHistoryKind::AgentMessageDelta,
                Some("fine".to_owned())
            ),
            (koduck_ai::application::ProviderHistoryKind::Terminal, None),
        ],
        "the independent request carries its own effective view"
    );
}

/// Real-`SQL` corruption: a stored payload that cannot decode rejects resume
/// as `Unavailable` on both routes, keeps rejecting unchanged, leaves the
/// rows untouched, and never blocks a separate valid Thread.
fn real_sql_corruption_persists_and_neighbor_thread_succeeds() {
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let corrupt_thread = seed_corrupt_payload_thread(&runtime, &pool, &tenant, &subject);
    let provider = RecordingProvider::new();
    let history = PostgresTurnHistory::new(executor.clone());
    let mut runner = TurnRunner::new(provider.clone(), history);

    let result = runner.execute(
        TurnCommand::new(owner.clone(), Some(corrupt_thread), "resume")
            .expect("valid resumed command"),
    );
    assert!(matches!(
        result,
        Err(TurnRunError::History(HistoryError::Unavailable))
    ));
    assert_eq!(sql_turn_count(&runtime, &pool, &tenant, corrupt_thread), 1);
    assert!(provider.recorded().is_empty());

    let mut adapter = HttpAdapter::new(TurnRunner::new(
        provider.clone(),
        PostgresTurnHistory::new(executor.clone()),
    ));
    for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
        let response = adapter.handle(chat_request(path, corrupt_thread, &owner));
        assert_eq!(response.status, 503, "path {path}");
        assert!(
            response
                .body
                .contains("\"code\":\"durability-unavailable\"")
        );
    }
    let mut emitted = Vec::new();
    let response = adapter.handle_stream_controlled(
        chat_request("/api/v1/ai/chat/stream", corrupt_thread, &owner),
        &mut |event| emitted.push(event),
        &|| false,
    );
    assert_eq!(
        response.status, 503,
        "the SSE route rejects before any stream"
    );
    assert!(emitted.is_empty(), "no SSE event is emitted");

    let valid_thread = seed_valid_thread(&executor, &owner);
    let response = adapter.handle(chat_request("/api/v1/ai/chat", valid_thread, &owner));
    assert_eq!(
        response.status, 200,
        "the separate valid Thread succeeds on the same adapter"
    );
    assert_eq!(response.header("Content-Type"), Some("application/json"));
    assert_eq!(
        sql_turn_count(&runtime, &pool, &tenant, corrupt_thread),
        1,
        "the unchanged corrupt Thread still accepts no new Turn"
    );
    runtime.block_on(pool.close());
}

/// Seeds one owned Thread whose single user row carries an undecodable payload.
fn seed_corrupt_payload_thread(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    subject: &str,
) -> ThreadId {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let owner = owner_trust(tenant, subject);
    seed_thread_row(runtime, pool, &owner, thread_id);
    seed_turn_row(
        runtime,
        pool,
        tenant,
        thread_id,
        turn_id,
        "2026-01-01T00:00:00+00:00",
        "completed",
    );
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
                 item_type, payload) VALUES ($1, $2, $3, 1, $4, 'user_message', 'not-json')",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(uuid::Uuid::new_v4())
            .execute(pool),
        )
        .expect("seed the corrupt payload row");
    thread_id
}

/// Seeds one valid terminal corrected Thread through the production executor.
fn seed_valid_thread(executor: &SqlxPostgresExecutor, owner: &TrustContext) -> ThreadId {
    let accepted = executor
        .accept_initial(&TurnCommand::new(owner.clone(), None, "valid").expect("valid command"))
        .expect("seed the valid Thread");
    executor
        .append(
            &accepted,
            koduck_ai::application::NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            }),
        )
        .expect("seed the valid terminal");
    accepted.thread_id
}

/// Builds one chat request body for the given path and Thread.
fn chat_request(path: &str, thread: ThreadId, owner: &TrustContext) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Post,
        path: path.to_owned(),
        content_type: Some("application/json".to_owned()),
        body: format!(r#"{{"input":"hello","thread_id":"{}"}}"#, thread.as_uuid()),
        trust: Some(owner.clone()),
    }
}
