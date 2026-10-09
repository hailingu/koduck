// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-4: two current Tool rounds preserve causality and reuse the same
//! corrected history snapshot. A barrier commits a second correction of the
//! source Turn's user root after provider request 1 and before request 2; the
//! continuation requests must carry the original snapshot, only
//! already-committed current-generation rounds, and the next independent
//! preparation must see the later correction.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Barrier};

use koduck_ai::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use koduck_ai::application::{
    ModelToolCall, ModelToolResult, ToolCallExecutor, ToolCallTurnContext, ToolProjection,
    ToolProjectionSink, TurnCommand, TurnRunner,
};
use koduck_ai::domain::execution::AttemptId;
use koduck_ai::domain::{Item, ItemId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage};

use super::support::*;
use crate::fixtures::*;

/// The synthetic C-5 boundary: records every serviced call, synchronizes the
/// mid-Turn correction commit with the loopback upstream through a barrier,
/// and emits one durable dispatch/result projection pair per call.
struct SnapshotToolExecutor {
    calls: Rc<RefCell<Vec<String>>>,
    history: MemoryHistory,
    barrier: Arc<Barrier>,
    correction: Option<(TurnId, ItemId)>,
}

impl SnapshotToolExecutor {
    fn clone_shared(&self) -> Self {
        Self {
            calls: Rc::clone(&self.calls),
            history: self.history.clone(),
            barrier: Arc::clone(&self.barrier),
            correction: self.correction,
        }
    }

    fn serviced_calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl ToolCallExecutor for SnapshotToolExecutor {
    fn execute_tool_call(
        &mut self,
        call: ModelToolCall,
        _context: &ToolCallTurnContext,
        _trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<ModelToolResult, koduck_ai::application::ToolCallError> {
        let first_serviced = self.calls.borrow().is_empty();
        self.calls.borrow_mut().push(call.name.clone());
        if first_serviced && let Some((turn, target)) = self.correction {
            // The barrier pairs with the upstream thread after it served
            // request 1: the second correction commits strictly between
            // provider request 1 and the continuation request 2.
            self.barrier.wait();
            self.history.inject_correction(
                turn,
                target,
                "revised-2",
                Some("correction2_committed"),
            );
        }
        let attempt_id = AttemptId::new();
        emit_projection(
            projections,
            &ToolProjection::ToolCall {
                descriptor_id: call.name,
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Running,
                version: 2,
            },
        );
        emit_projection(
            projections,
            &ToolProjection::ToolResult {
                attempt_id,
                status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
                code: None,
                effect_state: koduck_ai::application::EffectState::Started,
                output_bytes: 2,
                output_digest: Some(koduck_ai::application::output_digest(b"ok")),
                version: 3,
            },
        );
        Ok(ModelToolResult {
            content: "ok".to_owned(),
            is_error: false,
        })
    }
}

/// Appends and publishes one projection, mirroring the production emit order.
fn emit_projection(sink: &mut dyn ToolProjectionSink, projection: &ToolProjection) {
    sink.append(projection).expect("fixture projection appends");
    sink.publish(projection);
}

/// Runs the complete AC-4 check.
pub(crate) fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("AC-4 transport runtime");
    let responses = vec![
        tool_round_frames("Checking.", "fixture.tool"),
        tool_round_frames("", "second.tool"),
        completion_frames("Done."),
        completion_frames("A"),
    ];
    let log = event_log();
    let barrier = Arc::new(Barrier::new(2));
    let (base_url, receiver) = scripted_sse_upstream(
        responses,
        Some(Arc::clone(&log)),
        "ac4",
        Some(Arc::clone(&barrier)),
    );
    let transport = ReqwestOpenAiTransport::new(
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client"),
        runtime.handle().clone(),
        &base_url,
        "test-model",
        "test-key",
    );

    let history = MemoryHistory::with_event_log(Arc::clone(&log));
    let trust = command_trust();
    let thread_id = ThreadId::new();
    let source_turn = TurnId::new();
    let (source_items, correction_one_tip) = seeded_source_turn();
    history.seed_turn(thread_id, source_turn, source_items.clone());
    let source_before = history.items_of(source_turn);

    let executor = SnapshotToolExecutor {
        calls: Rc::new(RefCell::new(Vec::new())),
        history: history.clone(),
        barrier,
        correction: Some((source_turn, correction_one_tip)),
    };
    let mut runner = TurnRunner::new(OpenAiCompatibleProvider::new(transport), history.clone())
        .with_tool_executor(executor.clone_shared());

    let result = runner
        .execute(
            TurnCommand::new(trust.clone(), Some(thread_id), "question")
                .expect("valid resumed command"),
        )
        .expect("the two-round Tool Turn completes");

    assert_two_round_requests(
        &receiver,
        &executor,
        &history,
        result.turn_id,
        &log,
        source_turn,
        &source_before,
    );
    assert_next_preparation(&mut runner, &trust, thread_id, &receiver);
    drop(runtime);
}

/// Captures the two-round Turn's three requests and asserts the frozen
/// snapshot, causal rounds, dispatch scope, commit order, and raw source.
fn assert_two_round_requests(
    receiver: &std::sync::mpsc::Receiver<String>,
    executor: &SnapshotToolExecutor,
    history: &MemoryHistory,
    turn_id: TurnId,
    log: &EventLog,
    source_turn: TurnId,
    source_before: &[Item],
) {
    let bodies = captured_bodies(receiver, 3);
    assert_prior_context_is_frozen_and_current_input_once(&bodies);
    assert_round_causality(&bodies);
    assert_no_historical_dispatch_and_one_terminal(executor, history, turn_id);
    assert_commit_order(log);
    assert_source_snapshot(history, source_turn, source_before);
}

/// Runs the follow-up resume and asserts the next independent preparation
/// sees the second correction.
fn assert_next_preparation(
    runner: &mut TurnRunner<
        OpenAiCompatibleProvider<ReqwestOpenAiTransport>,
        MemoryHistory,
        SnapshotToolExecutor,
    >,
    trust: &TrustContext,
    thread_id: ThreadId,
    receiver: &std::sync::mpsc::Receiver<String>,
) {
    let after = runner
        .execute(
            TurnCommand::new(trust.clone(), Some(thread_id), "after")
                .expect("valid follow-up command"),
        )
        .expect("the next independent preparation completes");
    let bodies = captured_bodies(receiver, 1);
    assert_eq!(
        messages_of(&bodies[0]),
        vec![
            serde_json::json!({ "role": "user", "content": "revised-2" }),
            serde_json::json!({ "role": "assistant", "content": "answer" }),
            serde_json::json!({ "role": "user", "content": "question" }),
            serde_json::json!({ "role": "assistant", "content": "Checking.Done." }),
            serde_json::json!({ "role": "user", "content": "after" }),
        ],
        "the next independent preparation sees the second correction and the settled Tool Turn (turn {})",
        after.turn_id.as_uuid()
    );
}

/// Seeds the terminal source Turn with inert historical Tool views and one
/// post-terminal correction of its user root; returns the correction's own
/// identity so the mid-Turn second correction can extend that chain.
fn seeded_source_turn() -> (Vec<Item>, ItemId) {
    let root = user_item(1, "draft");
    let correction = correction_item(6, root.item_id, "revised-1");
    let chain_tip = correction.item_id;
    (
        vec![
            root,
            delta_item(2, "answer"),
            tool_call_item(3),
            tool_result_item(4),
            terminal_item(
                5,
                TerminalOutcome::Completed {
                    usage: Usage::new(1, 1).expect("valid usage"),
                },
            ),
            correction,
        ],
        chain_tip,
    )
}

/// Asserts every request carries the identical frozen prior context once and
/// the current input exactly once before any Tool round.
fn assert_prior_context_is_frozen_and_current_input_once(bodies: &[String]) {
    let frozen = [
        serde_json::json!({ "role": "user", "content": "revised-1" }),
        serde_json::json!({ "role": "assistant", "content": "answer" }),
    ];
    for (index, body) in bodies.iter().enumerate() {
        let messages = messages_of(body);
        assert_eq!(&messages[..2], &frozen[..], "request {}", index + 1);
        let current = messages
            .iter()
            .filter(|message| message["role"] == "user" && message["content"] == "question")
            .count();
        assert_eq!(
            current,
            1,
            "request {} carries the current input once",
            index + 1
        );
        let position = messages
            .iter()
            .position(|message| message["role"] == "user" && message["content"] == "question")
            .expect("the current input exists");
        assert_eq!(
            position, 2,
            "the current input directly follows prior history"
        );
    }
}

/// Asserts the continuation requests carry only committed current-generation
/// rounds in exact causal order with matching call identities.
fn assert_round_causality(bodies: &[String]) {
    let round_one = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "Checking.",
            "tool_calls": [{
                "id": "call_0",
                "type": "function",
                "function": { "name": "fixture.tool", "arguments": "{}" }
            }]
        }),
        serde_json::json!({ "role": "tool", "tool_call_id": "call_0", "content": "ok" }),
    ];
    let round_two = vec![
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "second.tool", "arguments": "{}" }
            }]
        }),
        serde_json::json!({ "role": "tool", "tool_call_id": "call_1", "content": "ok" }),
    ];
    let request_two = messages_of(&bodies[1]);
    assert_eq!(
        request_two[3..],
        round_one[..],
        "request 2 carries only round one"
    );
    let request_three = messages_of(&bodies[2]);
    let mut expected_tail = round_one.clone();
    expected_tail.extend_from_slice(&round_two);
    assert_eq!(
        request_three[3..],
        expected_tail[..],
        "request 3 carries both committed rounds in causal order"
    );
}

/// Asserts only the two current-generation calls were dispatched and the
/// current Turn ends in exactly one terminal.
fn assert_no_historical_dispatch_and_one_terminal(
    executor: &SnapshotToolExecutor,
    history: &MemoryHistory,
    turn_id: TurnId,
) {
    assert_eq!(
        executor.serviced_calls(),
        vec!["fixture.tool".to_owned(), "second.tool".to_owned()],
        "no historical Tool view is ever dispatched"
    );
    let items = history.items_of(turn_id);
    let projections = items
        .iter()
        .filter(|item| {
            matches!(
                item.payload,
                koduck_ai::domain::ItemPayload::ToolCall { .. }
                    | koduck_ai::domain::ItemPayload::ToolResult { .. }
            )
        })
        .count();
    assert_eq!(
        projections, 4,
        "both current rounds committed their projections"
    );
    assert_eq!(
        terminals_of(&items),
        vec![TerminalOutcome::Completed {
            usage: Usage::new(1, 1).expect("valid usage"),
        }],
        "exactly one terminal wins"
    );
}

/// Asserts the canonical commit order: request 1 precedes the second
/// correction, whose commit precedes the committed round-one results and
/// request 2.
fn assert_commit_order(log: &EventLog) {
    let entries = log_snapshot(log);
    let position = |name: &str| {
        entries
            .iter()
            .position(|entry| entry == name)
            .unwrap_or_else(|| panic!("missing ordering entry {name} in {entries:?}"))
    };
    let request_one = position("ac4_request_1");
    let correction = position("correction2_committed");
    let projection = entries
        .iter()
        .position(|entry| entry == "projection_appended")
        .unwrap_or_else(|| panic!("missing projection commit in {entries:?}"));
    let request_two = position("ac4_request_2");
    assert!(
        request_one < correction,
        "the correction commits after request 1"
    );
    assert!(
        correction < projection && projection < request_two,
        "the round-one results commit after the correction and before request 2"
    );
}

/// Asserts the source Turn's rows are unchanged except the deliberately
/// inserted second correction.
fn assert_source_snapshot(history: &MemoryHistory, source_turn: TurnId, before: &[Item]) {
    let after = history.items_of(source_turn);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        &after[..before.len()],
        before,
        "original rows are untouched"
    );
    assert!(
        matches!(&after[before.len()].payload, koduck_ai::domain::ItemPayload::Correction(link) if link.content() == "revised-2"),
        "the only source mutation is the deliberately inserted second correction"
    );
}
