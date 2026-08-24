// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest, ServiceError, TurnService};
use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiFrame, OpenAiFrameStream, OpenAiProtocolTransport,
    OpenAiTransportError,
};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnResult, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};

#[derive(Clone)]
struct FakeService {
    result: TurnResult,
    execute_calls: Rc<Cell<usize>>,
    interrupt_results: BTreeMap<TurnId, Result<(), ServiceError>>,
}

impl TurnService for FakeService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        self.execute_calls.set(self.execute_calls.get() + 1);
        Ok(self.result.clone())
    }

    fn interrupt(&mut self, _trust: &TrustContext, turn_id: TurnId) -> Result<(), ServiceError> {
        self.interrupt_results
            .get(&turn_id)
            .cloned()
            .unwrap_or(Err(ServiceError::NotFound))
    }
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context")
}

#[derive(Clone)]
struct ContextLimitService;

impl TurnService for ContextLimitService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        Err(ServiceError::InvalidRequest)
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Err(ServiceError::NotFound)
    }
}

#[test]
fn oversized_resume_context_uses_the_owned_invalid_request_problem() {
    let response = HttpAdapter::new(ContextLimitService).handle(HttpRequest {
        method: HttpMethod::Post,
        path: "/api/v1/ai/chat".to_owned(),
        content_type: Some("application/json".to_owned()),
        body: format!(
            "{{\"input\":\"hello\",\"thread_id\":\"{}\"}}",
            ThreadId::new().as_uuid()
        ),
        trust: Some(trust()),
    });

    assert_eq!(response.status, 400);
    assert!(response.body.contains("\"code\":\"invalid-request\""));
}

fn completed_result(deltas: &[&str]) -> TurnResult {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let usage = Usage::new(3, deltas.len() as u64).expect("valid usage");
    let mut replay = vec![Item::new(
        1,
        ItemPayload::UserMessage {
            content: "hello".to_owned(),
        },
    )];
    for delta in deltas {
        replay.push(Item::new(
            replay.len() as u64 + 1,
            ItemPayload::AgentMessageDelta {
                content: (*delta).to_owned(),
            },
        ));
    }
    replay.push(Item::new(
        replay.len() as u64 + 1,
        ItemPayload::Usage(usage),
    ));
    replay.push(Item::new(
        replay.len() as u64 + 1,
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
    ));
    let published = replay
        .iter()
        .filter(|item| {
            matches!(
                item.payload,
                ItemPayload::AgentMessageDelta { .. } | ItemPayload::Terminal(_)
            )
        })
        .cloned()
        .collect();
    TurnResult {
        thread_id,
        turn_id,
        status: TurnStatus::Completed,
        published,
        replay,
    }
}

fn adapter(result: TurnResult) -> HttpAdapter<FakeService> {
    HttpAdapter::new(FakeService {
        result,
        execute_calls: Rc::new(Cell::new(0)),
        interrupt_results: BTreeMap::new(),
    })
}

fn post(path: &str, body: &str, trust: Option<TrustContext>) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Post,
        path: path.to_owned(),
        content_type: Some("application/json".to_owned()),
        body: body.to_owned(),
        trust,
    }
}

#[test]
fn sync_chat_v1_contract() {
    let result = completed_result(&["A"]);
    let ids = result.clone();
    let mut adapter = adapter(result);

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 200);
    assert_eq!(response.header("Content-Type"), Some("application/json"));
    let normalized = response
        .body
        .replace(&ids.thread_id.as_uuid().to_string(), "{{thread_id}}")
        .replace(&ids.turn_id.as_uuid().to_string(), "{{turn_id}}")
        .replace(
            &ids.published[0].item_id.as_uuid().to_string(),
            "{{item_id}}",
        )
        .replace("\"input_tokens\":3", "\"input_tokens\":{{input_tokens}}")
        .replace("\"output_tokens\":1", "\"output_tokens\":{{output_tokens}}")
        .replace("\"total_tokens\":4", "\"total_tokens\":{{total_tokens}}");
    assert_eq!(
        normalized,
        include_str!("fixtures/sync-chat-v1.json").trim()
    );
}

#[test]
fn synchronous_failed_turn_returns_provider_unavailable() {
    let terminal = Item::new(
        2,
        ItemPayload::Terminal(TerminalOutcome::Failed {
            code: "UPSTREAM_RESET".to_owned(),
        }),
    );
    let mut adapter = adapter(TurnResult {
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        status: TurnStatus::Failed,
        published: vec![terminal.clone()],
        replay: vec![terminal],
    });

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 503);
    assert!(response.body.contains("\"code\":\"provider-unavailable\""));
}

#[test]
fn synchronous_non_completed_turn_is_not_a_success_response() {
    for (status, outcome, expected_code) in [
        (
            TurnStatus::Interrupted,
            TerminalOutcome::Interrupted,
            "turn-interrupted",
        ),
        (
            TurnStatus::Cancelled,
            TerminalOutcome::Cancelled,
            "turn-cancelled",
        ),
    ] {
        let terminal = Item::new(2, ItemPayload::Terminal(outcome));
        let mut adapter = adapter(TurnResult {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            status,
            published: vec![terminal.clone()],
            replay: vec![terminal],
        });

        let response = adapter.handle(post(
            "/api/v1/ai/chat",
            r#"{"input":"hello"}"#,
            Some(trust()),
        ));

        assert_eq!(response.status, 409);
        assert!(
            response
                .body
                .contains(&format!("\"code\":\"{expected_code}\""))
        );
    }
}

#[test]
fn json_media_type_parameters_are_accepted() {
    for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
        let mut request = post(path, r#"{"input":"hello"}"#, Some(trust()));
        request.content_type = Some("Application/JSON; charset=utf-8".to_owned());

        let response = adapter(completed_result(&["A"])).handle(request);

        assert_eq!(response.status, 200, "{path} accepts JSON parameters");
    }
}

#[test]
fn sse_v1_contract_and_append_before_publish() {
    let result = completed_result(&["A", "B"]);
    let durable = result.replay.clone();
    let published = result.published.clone();
    let ids = result.clone();
    let mut adapter = adapter(result);

    let response = adapter.handle(post(
        "/api/v1/ai/chat/stream",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 200);
    assert_eq!(response.header("Content-Type"), Some("text/event-stream"));
    assert_eq!(response.body.matches("event: turn.started").count(), 1);
    assert_eq!(response.body.matches("event: item.created").count(), 2);
    assert_eq!(response.body.matches("event: turn.completed").count(), 1);
    assert!(published.iter().all(|visible| {
        durable
            .iter()
            .any(|item| item.item_id == visible.item_id && item.sequence == visible.sequence)
    }));
    let sequences = response
        .body
        .lines()
        .filter_map(|line| line.split_once("\"sequence\":").map(|(_, tail)| tail))
        .filter_map(|tail| {
            tail.trim_end_matches('}')
                .split(',')
                .next()
                .and_then(|value| value.parse::<u64>().ok())
        })
        .collect::<Vec<_>>();
    assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(response.body.contains(&ids.thread_id.as_uuid().to_string()));
    assert!(response.body.contains(&ids.turn_id.as_uuid().to_string()));
    let normalized = response
        .body
        .replace(&ids.thread_id.as_uuid().to_string(), "{{thread_id}}")
        .replace(&ids.turn_id.as_uuid().to_string(), "{{turn_id}}")
        .replace(
            &ids.published[0].item_id.as_uuid().to_string(),
            "{{item_id_1}}",
        )
        .replace(
            &ids.published[1].item_id.as_uuid().to_string(),
            "{{item_id_2}}",
        )
        .replace("\"input_tokens\":3", "\"input_tokens\":{{input_tokens}}")
        .replace("\"output_tokens\":2", "\"output_tokens\":{{output_tokens}}")
        .replace("\"total_tokens\":5", "\"total_tokens\":{{total_tokens}}");
    assert_eq!(
        normalized.trim_end(),
        include_str!("fixtures/sse-v1.txt").trim_end()
    );
}

#[test]
fn interrupt_and_cancel_are_distinct() {
    let active_turn = TurnId::new();
    let terminal_turn = TurnId::new();
    let result = completed_result(&["A"]);
    let mut interrupt_results = BTreeMap::new();
    interrupt_results.insert(active_turn, Ok(()));
    interrupt_results.insert(terminal_turn, Err(ServiceError::AlreadyTerminal));
    let service = FakeService {
        result,
        execute_calls: Rc::new(Cell::new(0)),
        interrupt_results,
    };
    let mut interrupt_adapter = HttpAdapter::new(service);

    let accepted = interrupt_adapter.handle(post(
        &format!("/api/v1/ai/turns/{}/interrupt", active_turn.as_uuid()),
        "",
        Some(trust()),
    ));
    assert_eq!(accepted.status, 202);
    assert_eq!(
        accepted.body,
        format!(
            "{{\"turn_id\":\"{}\",\"status\":\"interrupt-requested\"}}",
            active_turn.as_uuid()
        )
    );

    let unknown = interrupt_adapter.handle(post(
        &format!("/api/v1/ai/turns/{}/interrupt", TurnId::new().as_uuid()),
        "",
        Some(trust()),
    ));
    assert_eq!(unknown.status, 404);
    let non_owned = interrupt_adapter.handle(post(
        &format!("/api/v1/ai/turns/{}/interrupt", TurnId::new().as_uuid()),
        "",
        Some(trust()),
    ));
    assert_eq!(non_owned.status, 404);
    assert_eq!(
        normalize_correlation_id(&unknown.body),
        normalize_correlation_id(&non_owned.body)
    );

    let terminal = interrupt_adapter.handle(post(
        &format!("/api/v1/ai/turns/{}/interrupt", terminal_turn.as_uuid()),
        "",
        Some(trust()),
    ));
    assert_eq!(terminal.status, 409);
    assert!(terminal.body.contains("\"code\":\"turn-already-terminal\""));

    let cancelled = TurnResult {
        status: TurnStatus::Cancelled,
        published: vec![Item::new(
            2,
            ItemPayload::Terminal(TerminalOutcome::Cancelled),
        )],
        replay: vec![
            Item::new(
                1,
                ItemPayload::UserMessage {
                    content: "hello".to_owned(),
                },
            ),
            Item::new(2, ItemPayload::Terminal(TerminalOutcome::Cancelled)),
        ],
        ..completed_result(&[])
    };
    let mut cancel_adapter = adapter(cancelled);
    let response = cancel_adapter.handle(post(
        "/api/v1/ai/chat/stream",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));
    assert!(response.body.contains("event: turn.cancelled"));
    assert!(!response.body.contains("event: turn.interrupted"));
    assert_kernel_interrupt();
}

#[test]
fn invalid_identity_stops_at_presentation_boundary() {
    let calls = Rc::new(Cell::new(0));
    let service = FakeService {
        result: completed_result(&["A"]),
        execute_calls: Rc::clone(&calls),
        interrupt_results: BTreeMap::new(),
    };
    let mut adapter = HttpAdapter::new(service);

    let response = adapter.handle(post("/api/v1/ai/chat", r#"{"input":"hello"}"#, None));

    assert_eq!(response.status, 401);
    assert_eq!(response.header("WWW-Authenticate"), Some("Bearer"));
    assert_eq!(
        response.header("Content-Type"),
        Some("application/problem+json")
    );
    assert!(response.body.contains("\"type\":\"about:blank\""));
    assert!(response.body.contains("\"title\":\"Invalid identity\""));
    assert!(response.body.contains("\"status\":401"));
    assert!(response.body.contains("\"code\":\"invalid-identity\""));
    assert_eq!(
        normalize_correlation_id(&response.body),
        include_str!("fixtures/invalid-identity-v1.json").trim()
    );
    assert_eq!(calls.get(), 0);
}

fn normalize_correlation_id(body: &str) -> String {
    let marker = "\"correlation_id\":\"";
    let start = body.find(marker).expect("problem has correlation id") + marker.len();
    let end = body[start..].find('"').expect("correlation id closes") + start;
    let value = &body[start..end];
    uuid::Uuid::parse_str(value).expect("correlation id is a UUID");
    body.replace(value, "{{correlation_id}}")
}

#[derive(Default)]
struct HistoryState {
    thread_items: BTreeMap<ThreadId, Vec<Item>>,
    turn_items: BTreeMap<TurnId, Vec<Item>>,
    interrupt_after_first_delta: bool,
}

#[derive(Clone, Default)]
struct SharedHistory {
    state: Rc<RefCell<HistoryState>>,
}

impl TurnHistory for SharedHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        self.state.borrow_mut().interrupt_after_first_delta = true;
        Ok(())
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        let state = self.state.borrow();
        Ok(state.interrupt_after_first_delta
            && state
                .turn_items
                .get(&turn.turn_id)
                .is_some_and(|items| items.len() >= 2))
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
        state
            .thread_items
            .entry(thread_id)
            .or_default()
            .push(input.clone());
        state.turn_items.insert(turn_id, vec![input.clone()]);
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.borrow_mut();
        let turn_items = state
            .turn_items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let durable = Item::new(turn_items.len() as u64 + 1, item.into_payload());
        turn_items.push(durable.clone());
        state
            .thread_items
            .get_mut(&turn.thread_id)
            .ok_or(HistoryError::NotFound)?
            .push(durable.clone());
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

    fn prior_thread_items(
        &self,
        _trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(self
            .state
            .borrow()
            .thread_items
            .get(&thread_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[derive(Clone)]
struct RecordingProvider {
    inputs: Rc<RefCell<Vec<ModelInput>>>,
}

impl ModelProvider for RecordingProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
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

#[test]
fn resume_creates_new_turn() {
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingProvider {
        inputs: Rc::clone(&inputs),
    };
    let history = SharedHistory::default();
    let history_probe = history.clone();
    let mut runner = TurnRunner::new(provider, history);
    let first = runner
        .execute(TurnCommand::new(trust(), None, "first").expect("valid first command"))
        .expect("first turn completes");
    let immutable_first = first.replay.clone();

    let second = runner
        .execute(
            TurnCommand::new(trust(), Some(first.thread_id), "second")
                .expect("valid resumed command"),
        )
        .expect("resumed turn completes");

    assert_eq!(second.thread_id, first.thread_id);
    assert_ne!(second.turn_id, first.turn_id);
    assert_eq!(
        history_probe
            .replay(&trust().tenant_id, first.turn_id)
            .expect("first replay remains readable"),
        immutable_first
    );
    let recorded = inputs.borrow();
    assert!(recorded[0].history.is_empty());
    assert_eq!(recorded[1].history, immutable_first);
}

fn assert_kernel_interrupt() {
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingProvider {
        inputs: Rc::clone(&inputs),
    };
    let history = SharedHistory::default();
    history.state.borrow_mut().interrupt_after_first_delta = true;
    let mut runner = TurnRunner::new(provider, history);

    let result = runner
        .execute(TurnCommand::new(trust(), None, "hello").expect("valid command"))
        .expect("interruption is a normal terminal result");

    assert_eq!(result.status, TurnStatus::Interrupted);
    assert!(matches!(
        result.replay.last().map(|item| &item.payload),
        Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
    ));
    assert_eq!(
        result
            .replay
            .iter()
            .filter(|item| matches!(item.payload, ItemPayload::Terminal(_)))
            .count(),
        1
    );
    assert!(!result.replay.iter().any(|item| matches!(
        item.payload,
        ItemPayload::Terminal(TerminalOutcome::Cancelled)
    )));
}

#[test]
fn http_adapter_executes_the_provider_neutral_kernel() {
    let provider = RecordingProvider {
        inputs: Rc::new(RefCell::new(Vec::new())),
    };
    let history = SharedHistory::default();
    let mut adapter = HttpAdapter::new(TurnRunner::new(provider, history));

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 200);
    assert!(response.body.contains("\"status\":\"completed\""));
}

/// Deterministic fixture sentinel standing in for one explicit transport
/// clean end (ADR-0004 PSC-1); appended after the final `data:` frame of a
/// scripted stream, it yields the ordered `OpenAiFrame::CleanEnd`.
const CLEAN_END: &str = "\u{0}clean-end";

/// Transport stub serving one scripted OpenAI-compatible frame stream per
/// request (ADR-0004).
#[derive(Clone, Default)]
struct OpenAiFrameTransport {
    scripts: Rc<RefCell<Vec<Vec<String>>>>,
}

impl OpenAiFrameTransport {
    fn scripted(scripts: Vec<Vec<&str>>) -> Self {
        Self {
            scripts: Rc::new(RefCell::new(
                scripts
                    .into_iter()
                    .map(|frames| frames.into_iter().map(str::to_owned).collect())
                    .collect(),
            )),
        }
    }
}

impl OpenAiProtocolTransport for OpenAiFrameTransport {
    fn chat_completion_frames(
        &mut self,
        _input: &ModelInput,
    ) -> Result<OpenAiFrameStream, OpenAiTransportError> {
        // One scripted frame stream per provider request; `remove` panics on
        // an exhausted script, mirroring the fixture contract.
        let frames = self.scripts.borrow_mut().remove(0);
        Ok(Box::new(frames.into_iter().map(|frame| {
            if frame == CLEAN_END {
                Ok(OpenAiFrame::CleanEnd)
            } else {
                Ok(OpenAiFrame::Data(frame))
            }
        })))
    }
}

/// `ModelProvider` running the production Chat Completions protocol
/// translation over scripted frame streams (ADR-0004).
#[derive(Clone)]
struct FrameScriptedProvider {
    inner: OpenAiCompatibleProvider<OpenAiFrameTransport>,
}

impl ModelProvider for FrameScriptedProvider {
    fn stream(&mut self, input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        self.inner.stream(input)
    }
}

#[test]
fn provider_completion_normalization_preserves_v1_delivery() {
    // ADR-0004 PSC-3/PSC-7: a `stop` finish plus optional usage and an
    // explicit clean end produces the exact existing synchronous completed v1
    // response, while an unannounced stream end retains the provider-failure
    // delivery mapping with no public field change.
    let transport = OpenAiFrameTransport::scripted(vec![vec![
        r#"data: {"choices":[{"delta":{"content":"A"},"finish_reason":"stop"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}"#,
        CLEAN_END,
    ]]);
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        FrameScriptedProvider {
            inner: OpenAiCompatibleProvider::new(transport),
        },
        SharedHistory::default(),
    ));

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 200);
    assert_eq!(response.header("Content-Type"), Some("application/json"));
    let value: serde_json::Value =
        serde_json::from_str(&response.body).expect("the completed body is JSON");
    let thread_id = value["thread_id"].as_str().expect("thread id").to_owned();
    let turn_id = value["turn_id"].as_str().expect("turn id").to_owned();
    let item_id = value["items"][0]["item_id"]
        .as_str()
        .expect("item id")
        .to_owned();
    let normalized = response
        .body
        .replace(&thread_id, "{{thread_id}}")
        .replace(&turn_id, "{{turn_id}}")
        .replace(&item_id, "{{item_id}}")
        .replace("\"input_tokens\":3", "\"input_tokens\":{{input_tokens}}")
        .replace("\"output_tokens\":1", "\"output_tokens\":{{output_tokens}}")
        .replace("\"total_tokens\":4", "\"total_tokens\":{{total_tokens}}");
    assert_eq!(
        normalized,
        include_str!("fixtures/sync-chat-v1.json").trim(),
        "the normalized clean-end completion equals the existing v1 golden fixture"
    );

    let transport = OpenAiFrameTransport::scripted(vec![vec![
        r#"data: {"choices":[{"delta":{"content":"A"},"finish_reason":"stop"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}"#,
        // No `[DONE]` and no explicit clean end: the stream ends unannounced.
    ]]);
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        FrameScriptedProvider {
            inner: OpenAiCompatibleProvider::new(transport),
        },
        SharedHistory::default(),
    ));

    let response = adapter.handle(post(
        "/api/v1/ai/chat",
        r#"{"input":"hello"}"#,
        Some(trust()),
    ));

    assert_eq!(response.status, 503);
    assert!(
        response.body.contains("\"code\":\"provider-unavailable\""),
        "an unannounced stream end retains the provider-failure delivery mapping"
    );
}
