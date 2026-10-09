// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-6: corrected input preserves the existing provider control outcomes.
//! Mid-stream persisted interruption and consumer disconnection each win
//! exactly one terminal with no late publication or post-terminal
//! continuation; a real `Reqwest` transport HTTP failure settles through the
//! existing failed-terminal path; a saturating frame burst keeps the bounded
//! channel and durable-before-visible ordering intact. The two inherited
//! provider timeout checks run separately through the library timing seam.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use koduck_ai::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use koduck_ai::application::{
    MAX_BUFFERED_DELTA_BYTES, ModelInput, ModelProvider, ProviderError, ProviderEvent, TurnCommand,
    TurnRunner, TurnStreamEvent,
};
use koduck_ai::domain::{Item, ItemPayload, TerminalOutcome, ThreadId, TurnId, Usage};

use super::support::*;
use crate::fixtures::*;

/// Provider double that stalls forever after one delta, so the persisted
/// interruption poll and the consumer cancellation check drive the outcome.
#[derive(Clone)]
struct StallingProvider {
    streams: Rc<RefCell<usize>>,
}

impl StallingProvider {
    fn new() -> Self {
        Self {
            streams: Rc::new(RefCell::new(0)),
        }
    }

    fn stream_count(&self) -> usize {
        *self.streams.borrow()
    }
}

impl ModelProvider for StallingProvider {
    fn stream(
        &mut self,
        _input: ModelInput,
    ) -> Result<koduck_ai::application::ProviderStream<'_>, ProviderError> {
        *self.streams.borrow_mut() += 1;
        Ok(Box::new(
            std::iter::once(ProviderEvent::Delta("A".to_owned()))
                .chain(std::iter::repeat(ProviderEvent::Pending)),
        ))
    }
}

/// Provider double emitting a bounded delta sequence then completing.
struct FiniteProvider {
    deltas: usize,
}

impl ModelProvider for FiniteProvider {
    fn stream(
        &mut self,
        _input: ModelInput,
    ) -> Result<koduck_ai::application::ProviderStream<'_>, ProviderError> {
        Ok(Box::new(
            (0..self.deltas)
                .map(|_| ProviderEvent::Delta("A".to_owned()))
                .chain(std::iter::once(ProviderEvent::Completed))
                .collect::<Vec<_>>()
                .into_iter(),
        ))
    }
}

/// Runs the complete AC-6 integration check; the two inherited library
/// timeout checks run through their own exact commands.
pub(crate) fn run() {
    interrupted_mid_stream_wins_exactly_one_terminal();
    disconnected_consumer_cancels_without_late_publication();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("AC-6 transport runtime");
    http_failure_settles_through_the_failed_terminal(&runtime);
    saturating_frame_burst_keeps_bounds_and_ordering(&runtime);
    drop(runtime);
}

/// Seeds one terminal corrected source Turn and returns its Thread.
fn corrected_source(history: &MemoryHistory) -> ThreadId {
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let root = user_item(1, "draft");
    let target = root.item_id;
    history.seed_turn(
        thread,
        turn,
        vec![
            root,
            delta_item(2, "answer"),
            terminal_item(
                3,
                TerminalOutcome::Completed {
                    usage: Usage::new(1, 1).expect("valid usage"),
                },
            ),
            correction_item(4, target, "revised"),
        ],
    );
    thread
}

/// A persisted interrupt requested mid-stream wins exactly one Interrupted
/// terminal; nothing is published after it and no continuation starts.
fn interrupted_mid_stream_wins_exactly_one_terminal() {
    let history = MemoryHistory::default();
    let thread = corrected_source(&history);
    let flag = Arc::new(AtomicBool::new(false));
    let runner_history = history.clone();
    let interrupted_history = runner_history.with_interrupt_flag(Arc::clone(&flag));
    let provider = StallingProvider::new();
    let mut runner = TurnRunner::new(provider.clone(), interrupted_history);
    let turn = Arc::new(std::sync::Mutex::new(None::<TurnId>));
    let observed_turn = Arc::clone(&turn);
    let mut events = Vec::new();
    let handle = {
        let flag = Arc::clone(&flag);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            flag.store(true, Ordering::SeqCst);
        })
    };
    let result = runner.execute_with_observer(
        TurnCommand::new(command_trust(), Some(thread), "resume").expect("valid command"),
        &mut |event: TurnStreamEvent| {
            if let TurnStreamEvent::Started { turn_id, .. } = event {
                *observed_turn.lock().expect("turn lock") = Some(turn_id);
            }
            events.push(event);
        },
    );
    handle.join().expect("the interrupt flag thread joins");

    let result = result.expect("the interrupted Turn settles normally");
    assert_eq!(result.status, koduck_ai::domain::TurnStatus::Interrupted);
    let turn_id = turn.lock().expect("turn lock").expect("the Turn started");
    let items = history.items_of(turn_id);
    assert_eq!(
        terminals_of(&items),
        vec![TerminalOutcome::Interrupted],
        "exactly one Interrupted terminal wins"
    );
    assert_eq!(
        provider.stream_count(),
        1,
        "no post-terminal continuation starts"
    );
    let observed = observed_items(&events);
    assert_eq!(
        observed.last(),
        items.last(),
        "the terminal is the last publication"
    );
    assert!(
        observed.len() <= items.len(),
        "nothing is published beyond the durable rows"
    );
}

/// A consumer disconnect mid-stream cancels the Turn with exactly one
/// terminal and no late publication.
fn disconnected_consumer_cancels_without_late_publication() {
    let history = MemoryHistory::default();
    let thread = corrected_source(&history);
    let cancellations = Arc::new(AtomicUsize::new(0));
    let mut runner = TurnRunner::new(FiniteProvider { deltas: 20 }, history.clone());
    let mut events = Vec::new();
    let result = runner.execute_with_observer_and_cancellation(
        TurnCommand::new(command_trust(), Some(thread), "resume").expect("valid command"),
        &mut |event: TurnStreamEvent| events.push(event),
        &|| cancellations.fetch_add(1, Ordering::SeqCst) >= 2,
    );

    let result = result.expect("the cancelled Turn settles normally");
    assert_eq!(result.status, koduck_ai::domain::TurnStatus::Cancelled);
    assert_eq!(
        terminals_of(&result.replay),
        vec![TerminalOutcome::Cancelled],
        "exactly one Cancelled terminal wins"
    );
    let observed = observed_items(&events);
    assert_eq!(
        observed, result.published,
        "publication stays durable-before-visible"
    );
    assert_eq!(
        observed.last(),
        result.replay.last(),
        "the terminal is the last publication"
    );
}

/// A real `Reqwest` HTTP failure settles through the existing failed-terminal
/// path while the corrected context is already prepared.
fn http_failure_settles_through_the_failed_terminal(runtime: &tokio::runtime::Runtime) {
    let (base_url, receiver) =
        scripted_sse_upstream(vec![http_failure_response(500)], None, "ac6", None);
    let history = MemoryHistory::default();
    let thread = corrected_source(&history);
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
    let mut runner = TurnRunner::new(OpenAiCompatibleProvider::new(transport), history.clone());

    let result = runner
        .execute(TurnCommand::new(command_trust(), Some(thread), "resume").expect("valid command"))
        .expect("the failed transport settles through its terminal");

    assert_eq!(result.status, koduck_ai::domain::TurnStatus::Failed);
    assert_eq!(
        terminals_of(&result.replay),
        vec![TerminalOutcome::Failed {
            code: "OPENAI_HTTP_500".to_owned()
        }],
        "the typed transport failure code owns the terminal"
    );
    let request = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("the upstream captured the request");
    assert_eq!(
        messages_of(&request)[0],
        serde_json::json!({ "role": "user", "content": "revised" }),
        "the corrected context was prepared before the failure"
    );
}

/// A burst larger than the transport's bounded frame channel completes with
/// bounded durable deltas and unchanged append-before-publish ordering.
fn saturating_frame_burst_keeps_bounds_and_ordering(runtime: &tokio::runtime::Runtime) {
    let frames = saturating_frames(300, 100);
    let (base_url, receiver) = scripted_sse_upstream(vec![frames], None, "ac6", None);
    let history = MemoryHistory::default();
    let thread = corrected_source(&history);
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
    let mut runner = TurnRunner::new(OpenAiCompatibleProvider::new(transport), history.clone());
    let mut events = Vec::new();

    let result = runner
        .execute_with_observer(
            TurnCommand::new(command_trust(), Some(thread), "resume").expect("valid command"),
            &mut |event: TurnStreamEvent| events.push(event),
        )
        .expect("the saturated stream completes");

    assert_eq!(result.status, koduck_ai::domain::TurnStatus::Completed);
    let assistant: String = result
        .replay
        .iter()
        .filter_map(delta_content)
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(
        assistant.len(),
        300 * 100,
        "the bounded channel loses no frame"
    );
    for item in &result.replay {
        if let Some(content) = delta_content(item) {
            assert!(
                content.len() <= MAX_BUFFERED_DELTA_BYTES,
                "durable deltas stay within the bounded chunk size"
            );
        }
    }
    let observed = observed_items(&events);
    assert_eq!(
        observed, result.published,
        "publication stays durable-before-visible"
    );
    assert_eq!(
        observed.last(),
        result.replay.last(),
        "the terminal is the last publication"
    );
    let _ = receiver.recv_timeout(Duration::from_secs(5));
}

/// Builds one scripted response whose frame count exceeds the transport's
/// bounded channel capacity.
fn saturating_frames(burst: usize, size: usize) -> Vec<u8> {
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    let chunk = "b".repeat(size);
    for _ in 0..burst {
        let frame = format!(
            "data: {}",
            serde_json::json!({ "choices": [{ "delta": { "content": chunk } }] })
        );
        response.extend_from_slice(frame.as_bytes());
        response.extend_from_slice(b"\n\n");
    }
    response.extend_from_slice(br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
    response.extend_from_slice(b"\n\n");
    response.extend_from_slice(br#"data: {"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#);
    response.extend_from_slice(b"\n\n");
    response.extend_from_slice(b"data: [DONE]\n\n");
    response
}

/// Returns the delta content of one Item, when it is a delta.
fn delta_content(item: &Item) -> Option<String> {
    match &item.payload {
        ItemPayload::AgentMessageDelta { content } => Some(content.clone()),
        _ => None,
    }
}
