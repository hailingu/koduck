// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! CAND-13 focused tests for the typed provider-context preparation seam
//! (ADR-0006 PC-02 through PC-04): complete per-Turn groups with explicit
//! source provenance are validated atomically, projected through the
//! unchanged CAND-12 owner, and converted into one owned effective provider
//! view. The AC-1 through AC-7 acceptance checks live in focused child
//! modules of this module inside the coverage-selected integration target;
//! each named test below is the single executable ADR acceptance entry.

#[path = "cand_13_context/atomic_rejection.rs"]
mod atomic_rejection;
#[path = "cand_13_context/bounds_deadline.rs"]
mod bounds_deadline;
#[path = "cand_13_context/controls_failures.rs"]
mod controls_failures;
#[path = "cand_13_context/effective_messages.rs"]
mod effective_messages;
#[path = "cand_13_context/scoped_read.rs"]
mod scoped_read;
#[path = "cand_13_context/support.rs"]
mod support;
#[path = "cand_13_context/tool_snapshot.rs"]
mod tool_snapshot;
#[path = "cand_13_context/trust_problems.rs"]
mod trust_problems;

use std::time::Duration;

use super::fixtures::*;
use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest};
use koduck_ai::adapters::provider::{OpenAiProtocolTransport, ReqwestOpenAiTransport};
use koduck_ai::application::{
    HistoryError, PriorTurnHistory, ProjectionError, ProviderContextError, ProviderHistoryItem,
    ProviderHistoryKind, ProviderHistoryValue, TurnCommand, TurnHistory, TurnRunError, TurnRunner,
    prepare_provider_history,
};
use koduck_ai::domain::{TerminalOutcome, ThreadId, TrustContext, TurnId};
use support::*;

/// AC-1 (ADR-0006): the production read preserves actual per-Turn provenance
/// and canonical order without modifying source history.
#[test]
fn scoped_thread_read() {
    scoped_read::run();
}

/// AC-2 (ADR-0006): actual provider requests contain the ordered effective
/// messages with no correction duplicates.
#[test]
fn effective_provider_messages() {
    effective_messages::run();
}

/// AC-3 (ADR-0006): invalid source anywhere rejects the whole prepared
/// context before acceptance or dispatch without contaminating later
/// independent requests.
#[test]
fn atomic_context_rejection() {
    atomic_rejection::run();
}

/// AC-4 (ADR-0006): two current Tool rounds preserve causality and reuse the
/// same corrected history snapshot.
#[test]
fn tool_continuation_snapshot() {
    tool_snapshot::run();
}

/// AC-5 (ADR-0006): raw admission boundaries remain exact, derived history
/// cannot expand, and the database deadline rejects without acceptance.
#[test]
fn context_bounds_and_deadline() {
    bounds_deadline::run();
}

/// AC-6 (ADR-0006): corrected input preserves existing provider timeout,
/// interruption, cancellation, and backpressure outcomes.
#[test]
fn controls_and_transport_failures() {
    controls_failures::run();
}

/// AC-7 (ADR-0006): trust rejection and context errors retain exact v1
/// problems without leaking source values.
#[test]
fn trust_and_problem_contract() {
    trust_problems::run();
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
    assert_eq!(prepared[0].effective_text(), Some("revised"));
    assert_eq!(prepared[0].source_item_id, correction.item_id);
    assert_eq!(prepared[1].item_id, answer.item_id);
    assert_eq!(prepared[1].kind, ProviderHistoryKind::AgentMessageDelta);
    assert_eq!(prepared[1].effective_text(), Some("answer"));
    assert_eq!(prepared[1].source_item_id, answer.item_id);
    assert_eq!(prepared[2].kind, ProviderHistoryKind::Terminal);
    assert_eq!(prepared[2].effective_text(), None);
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
    let inert_payloads = vec![
        usage.payload.clone(),
        call.payload.clone(),
        result.payload.clone(),
        approval.payload.clone(),
    ];

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
    assert_eq!(prepared[0].effective_text(), Some("first"));
    assert_eq!(prepared[0].source_item_id, first_item.item_id);
    assert_eq!(prepared[1].kind, ProviderHistoryKind::Usage);
    assert_eq!(prepared[2].kind, ProviderHistoryKind::ToolCall);
    assert_eq!(prepared[3].kind, ProviderHistoryKind::ToolResult);
    assert_eq!(prepared[4].kind, ProviderHistoryKind::ApprovalStatus);
    // Every non-text view retains its unchanged original payload value
    // alongside the kind semantics (PC-04).
    for (view, payload) in prepared[1..=4].iter().zip(&inert_payloads) {
        assert_eq!(view.effective_text(), None);
        assert_eq!(view.value, ProviderHistoryValue::NonText(payload.clone()));
    }
    assert_eq!(prepared[5].item_id, second_item.item_id);
    assert_eq!(prepared[5].effective_text(), Some("second"));
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

/// PC-01/PC-02 through real `SQLx`: the bounded read returns complete
/// Turn-scoped groups with actual per-row provenance — including a
/// correction committed after a terminal — and preparation turns them into
/// the ordered effective view. The full order/limit/deadline matrix is the
/// AC-1/AC-5 child modules; this smoke proves the replaced seam end to end.
#[test]
fn production_read_groups_real_turns_for_preparation() {
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    let seeded = seed_two_turn_thread(&executor);
    let owner = owner_trust(&seeded.first.tenant_id, "owner");

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
                view.effective_text(),
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

/// PC-02/PC-04 through the runner: a resumed Turn prepares the effective
/// view before acceptance, sends it as `ModelInput.history`, and leaves the
/// canonical raw rows unchanged.
#[test]
fn runner_resumes_from_the_prepared_effective_history() {
    let provider = RecordingProvider::new();
    let history = MemoryHistory::default();
    let mut runner = TurnRunner::new(provider.clone(), history.clone());
    let first = runner
        .execute(TurnCommand::new(command_trust(), None, "draft").expect("valid command"))
        .expect("first Turn completes");
    let immutable_first = first.replay.clone();
    let correction =
        history.inject_correction(first.turn_id, immutable_first[0].item_id, "revised", None);

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
    let recorded = provider.recorded();
    assert!(
        recorded[0].history.is_empty(),
        "a new Thread carries no prior context"
    );
    assert_eq!(
        recorded[1]
            .history
            .iter()
            .map(|view| (
                view.item_id,
                view.kind,
                view.effective_text(),
                view.value.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                immutable_first[0].item_id,
                ProviderHistoryKind::UserMessage,
                Some("revised"),
                ProviderHistoryValue::Text("revised".to_owned())
            ),
            (
                immutable_first[1].item_id,
                ProviderHistoryKind::AgentMessageDelta,
                Some("A"),
                ProviderHistoryValue::Text("A".to_owned())
            ),
            (
                immutable_first[2].item_id,
                ProviderHistoryKind::Usage,
                None,
                ProviderHistoryValue::NonText(immutable_first[2].payload.clone())
            ),
            (
                immutable_first[3].item_id,
                ProviderHistoryKind::Terminal,
                None,
                ProviderHistoryValue::NonText(immutable_first[3].payload.clone())
            ),
        ],
        "the resumed request carries the effective view once, with no correction entry"
    );
}

/// PC-07 at the HTTP boundary: a corrupt correction chain rejects resume as
/// the existing 503 problem before any acceptance, stream, or provider call.
#[test]
fn context_rejection_maps_to_the_503_problem_before_any_stream() {
    let provider = RecordingProvider::new();
    let history = MemoryHistory::default();
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    // A correction strictly precedes its target: the raw structure pass
    // accepts the reference, so ancestry reports the forward reference.
    let later_root = user_item(2, "later root");
    history.seed_turn(
        thread_id,
        turn_id,
        vec![
            correction_item(1, later_root.item_id, "early correction"),
            later_root,
        ],
    );
    let mut adapter = HttpAdapter::new(TurnRunner::new(provider.clone(), history.clone()));
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
    assert!(
        provider.recorded().is_empty(),
        "no provider request is made"
    );
}

/// PC-07 typed read failure: a pre-acceptance read `Unavailable` — read
/// deadline expiry or decode failure — surfaces as the exact History
/// variant, without the post-acceptance durability conversion.
#[test]
fn pre_acceptance_read_failure_retains_the_exact_history_variant() {
    let history = MemoryHistory::default();
    history.fail_next_prior_read(HistoryError::Unavailable);
    let mut runner = TurnRunner::new(RecordingProvider::new(), history.clone());

    let result = runner.execute(
        TurnCommand::new(command_trust(), Some(ThreadId::new()), "hello")
            .expect("valid resumed command"),
    );

    assert!(matches!(
        result,
        Err(TurnRunError::History(HistoryError::Unavailable))
    ));
    assert_eq!(history.accepted_count(), 0, "no Turn is accepted");
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
    let document = parse_request(&request);
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
    let (pool, runtime) = migrated_pool();
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let thread_id = ThreadId::new();
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO threads (tenant_id, subject_id, thread_id) VALUES ($1, $2, $3)",
            )
            .bind(tenant.as_str())
            .bind(subject.as_str())
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

    let intruder = TrustContext::new(tenant.clone(), "intruder").expect("intruder trust context");
    assert_eq!(
        TurnHistory::prior_thread_turns(&history, &intruder, thread_id),
        Err(HistoryError::NotFound),
        "a different subject observes no thread"
    );

    runtime.block_on(pool.close());
}
