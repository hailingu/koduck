// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::adapters::provider::{
    OpenAiCompatibleProvider, OpenAiProtocolTransport, ReqwestOpenAiTransport,
};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnResult, TurnRunner, TurnStreamEvent,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};
use koduck_ai::runtime::{RuntimeConfig, build_router};
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tower::ServiceExt;

/// Turn-only router fixture: no approval transport is configured, so any
/// approval-decision request observes the owned unavailability outcome.
#[derive(Clone)]
struct ApprovalsUnavailable;

impl koduck_ai::adapters::http::approvals::ApprovalDecisionTransport for ApprovalsUnavailable {
    fn decide(
        &mut self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
        _approval_id: koduck_ai::domain::execution::ApprovalId,
        _decision: koduck_ai::domain::execution::ApprovalDecision,
        _decided_at_millis: u64,
    ) -> koduck_ai::application::ApprovalDecisionOutcome {
        koduck_ai::application::ApprovalDecisionOutcome::Unavailable
    }
}

fn complete_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "KODUCK_AI_BIND_ADDR".to_owned(),
            "127.0.0.1:8080".to_owned(),
        ),
        (
            "KODUCK_AI_DATABASE_URL".to_owned(),
            "postgres://koduck@database/koduck".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
            "https://provider.example/v1".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_MODEL".to_owned(),
            "provider-model".to_owned(),
        ),
        (
            "KODUCK_AI_OPENAI_API_KEY".to_owned(),
            "not-a-real-secret".to_owned(),
        ),
    ])
}

#[test]
fn runtime_config_requires_postgres_and_provider_inputs() {
    let config = RuntimeConfig::from_environment(&complete_environment())
        .expect("complete runtime configuration is valid");

    assert_eq!(config.bind_addr().to_string(), "127.0.0.1:8080");
    assert_eq!(config.database_url(), "postgres://koduck@database/koduck");
    assert_eq!(config.provider_base_url(), "https://provider.example/v1");
    assert_eq!(config.provider_model(), "provider-model");
    assert_eq!(config.provider_api_key(), "not-a-real-secret");
    assert!(!format!("{config:?}").contains("not-a-real-secret"));

    let mut missing_database = complete_environment();
    missing_database.remove("KODUCK_AI_DATABASE_URL");
    assert_eq!(
        RuntimeConfig::from_environment(&missing_database)
            .expect_err("database URL is required")
            .to_string(),
        "missing required environment variable KODUCK_AI_DATABASE_URL"
    );
}

#[test]
fn runtime_config_rejects_unsafe_provider_base_url_components() {
    for unsafe_url in [
        "https://user:secret@provider.example/v1",
        "https://provider.example/v1?tenant=other",
        "https://provider.example/v1#fragment",
    ] {
        let mut environment = complete_environment();
        environment.insert(
            "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
            unsafe_url.to_owned(),
        );
        let error = RuntimeConfig::from_environment(&environment)
            .expect_err("unsafe provider base URL components are rejected");

        assert_eq!(error.to_string(), "invalid KODUCK_AI_OPENAI_BASE_URL");
        assert!(!format!("{error:?}").contains("secret"));
    }
}

#[test]
fn concrete_runtime_adapters_implement_owned_ports() {
    fn assert_history<T: PostgresExecutor + Send + Sync>() {}
    fn assert_provider<T: OpenAiProtocolTransport + Send>() {}

    assert_history::<SqlxPostgresExecutor>();
    assert_provider::<ReqwestOpenAiTransport>();
}

#[derive(Clone)]
struct StubService;

impl TurnService for StubService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        let usage = Usage::new(1, 1).expect("valid usage");
        let delta = Item::new(
            2,
            ItemPayload::AgentMessageDelta {
                content: "A".to_owned(),
            },
        );
        let terminal = Item::new(
            4,
            ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
        );
        Ok(TurnResult {
            thread_id: koduck_ai::domain::ThreadId::new(),
            turn_id: TurnId::new(),
            status: TurnStatus::Completed,
            published: vec![delta.clone(), terminal.clone()],
            replay: vec![delta, Item::new(3, ItemPayload::Usage(usage)), terminal],
        })
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn axum_router_hands_validated_identity_to_owned_http_adapter() {
    let router = build_router(StubService, ApprovalsUnavailable);
    let missing_identity = router
        .clone()
        .oneshot(
            Request::post("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"input":"hello"}"#))
                .expect("valid request"),
        )
        .await
        .expect("router response");
    assert_eq!(missing_identity.status(), StatusCode::UNAUTHORIZED);

    let response = router
        .oneshot(
            Request::post("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .header("x-koduck-tenant-id", "tenant-a")
                .header("x-koduck-subject-id", "subject-a")
                .body(Body::from(r#"{"input":"hello"}"#))
                .expect("valid request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("bounded response body");
    assert!(String::from_utf8_lossy(&body).contains("\"status\":\"completed\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_utf8_request_body_is_rejected() {
    let mut body = br#"{"input":"hel"#.to_vec();
    body.push(0xff);
    body.extend_from_slice(br#"lo"}"#);

    let response = build_router(StubService, ApprovalsUnavailable)
        .oneshot(
            Request::post("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .header("x-koduck-tenant-id", "tenant-a")
                .header("x-koduck-subject-id", "subject-a")
                .body(Body::from(body.clone()))
                .expect("valid request envelope"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let missing_identity = build_router(StubService, ApprovalsUnavailable)
        .oneshot(
            Request::post("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("valid request envelope"),
        )
        .await
        .expect("router response");
    assert_eq!(missing_identity.status(), StatusCode::UNAUTHORIZED);
}

#[derive(Clone)]
struct MidTurnFailureService;

impl TurnService for MidTurnFailureService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        Err(ServiceError::DurabilityUnavailable)
    }

    fn execute_stream(
        &mut self,
        _command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, ServiceError> {
        observer(TurnStreamEvent::Started {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
        });
        Err(ServiceError::DurabilityUnavailable)
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mid_turn_failure_is_reported_inside_an_started_sse_stream() {
    let response = build_router(MidTurnFailureService, ApprovalsUnavailable)
        .oneshot(chat_request("/api/v1/ai/chat/stream"))
        .await
        .expect("stream response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("bounded stream body");
    let body = String::from_utf8(body.to_vec()).expect("SSE is UTF-8");
    assert!(body.contains("event: turn.started"));
    assert!(body.contains("event: error"));
    assert!(body.contains("\"code\":\"durability-unavailable\""));
}

#[derive(Default)]
struct BlockingServiceState {
    started: bool,
    interrupted: bool,
}

#[derive(Clone, Default)]
struct BlockingService {
    state: Arc<(Mutex<BlockingServiceState>, Condvar)>,
}

impl BlockingService {
    fn wait_until_started(&self) {
        let (lock, ready) = &*self.state;
        let mut state = lock.lock().expect("blocking service state lock");
        while !state.started {
            state = ready.wait(state).expect("blocking service wait");
        }
    }

    fn release(&self) {
        let (lock, ready) = &*self.state;
        lock.lock()
            .expect("blocking service state lock")
            .interrupted = true;
        ready.notify_all();
    }
}

impl TurnService for BlockingService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        let (lock, ready) = &*self.state;
        let mut state = lock.lock().expect("blocking service state lock");
        state.started = true;
        ready.notify_all();
        while !state.interrupted {
            state = ready.wait(state).expect("blocking service wait");
        }
        Ok(completed_result())
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        self.release();
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_bypasses_an_active_turn() {
    let service = BlockingService::default();
    let router = build_router(service.clone(), ApprovalsUnavailable);
    let chat = tokio::spawn(router.clone().oneshot(chat_request("/api/v1/ai/chat")));
    tokio::task::spawn_blocking({
        let service = service.clone();
        move || service.wait_until_started()
    })
    .await
    .expect("started waiter joins");

    let interrupt = router.oneshot(interrupt_request(TurnId::new()));
    let response = if let Ok(response) = timeout(Duration::from_millis(250), interrupt).await {
        response.expect("interrupt response")
    } else {
        service.release();
        let _ = chat.await;
        panic!("interrupt waited for the active turn-wide mutex");
    };

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        chat.await
            .expect("chat task joins")
            .expect("chat response")
            .status(),
        StatusCode::OK
    );
}

#[derive(Clone)]
struct GatedProvider {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl GatedProvider {
    fn release(&self) {
        let (lock, ready) = &*self.gate;
        *lock.lock().expect("provider gate lock") = true;
        ready.notify_all();
    }
}

impl ModelProvider for GatedProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let gate = Arc::clone(&self.gate);
        let mut events = vec![
            ProviderEvent::Delta("A".to_owned()),
            ProviderEvent::Usage(Usage::new(1, 1).expect("valid usage")),
            ProviderEvent::Completed,
        ]
        .into_iter();
        Ok(Box::new(std::iter::from_fn(move || {
            let (lock, ready) = &*gate;
            let mut released = lock.lock().expect("provider gate lock");
            while !*released {
                released = ready.wait(released).expect("provider gate wait");
            }
            drop(released);
            events.next()
        })))
    }
}

#[derive(Default)]
struct ConcurrentHistoryState {
    items: BTreeMap<TurnId, Vec<Item>>,
    interrupts: BTreeSet<TurnId>,
}

#[derive(Clone, Default)]
struct ConcurrentHistory {
    state: Arc<Mutex<ConcurrentHistoryState>>,
}

impl TurnHistory for ConcurrentHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        turn_id: TurnId,
        _tool_terminals: Vec<koduck_ai::application::NewItem>,
    ) -> Result<(), HistoryError> {
        let mut state = self.state.lock().expect("history state lock");
        if !state.items.contains_key(&turn_id) {
            return Err(HistoryError::NotFound);
        }
        state.interrupts.insert(turn_id);
        Ok(())
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(self
            .state
            .lock()
            .expect("history state lock")
            .interrupts
            .contains(&turn.turn_id))
    }

    fn prior_thread_items(
        &self,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        Ok(Vec::new())
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        let turn_id = TurnId::new();
        let thread_id = command.thread_id.unwrap_or_default();
        let input = Item::new(
            1,
            ItemPayload::UserMessage {
                content: command.input.clone(),
            },
        );
        self.state
            .lock()
            .expect("history state lock")
            .items
            .insert(turn_id, vec![input.clone()]);
        Ok(AcceptedTurn::new(
            command.trust.tenant_id.clone(),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            input,
        ))
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        let mut state = self.state.lock().expect("history state lock");
        let items = state
            .items
            .get_mut(&turn.turn_id)
            .ok_or(HistoryError::NotFound)?;
        let durable = Item::new(items.len() as u64 + 1, item.into_payload());
        items.push(durable.clone());
        Ok(durable)
    }

    fn replay(&self, _tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.state
            .lock()
            .expect("history state lock")
            .items
            .get(&turn_id)
            .cloned()
            .ok_or(HistoryError::NotFound)
    }
}

impl ConcurrentHistory {
    fn accepted_turn_id(&self) -> TurnId {
        *self
            .state
            .lock()
            .expect("history state lock")
            .items
            .keys()
            .next()
            .expect("one turn was accepted")
    }

    fn item_count(&self, turn_id: TurnId) -> usize {
        self.state
            .lock()
            .expect("history state lock")
            .items
            .get(&turn_id)
            .map_or(0, Vec::len)
    }

    fn has_interrupted_terminal(&self, turn_id: TurnId) -> bool {
        self.state
            .lock()
            .expect("history state lock")
            .items
            .get(&turn_id)
            .is_some_and(|items| {
                matches!(
                    items.last().map(|item| &item.payload),
                    Some(ItemPayload::Terminal(TerminalOutcome::Interrupted))
                )
            })
    }

    fn has_cancelled_terminal(&self, turn_id: TurnId) -> bool {
        self.state
            .lock()
            .expect("history state lock")
            .items
            .get(&turn_id)
            .is_some_and(|items| {
                matches!(
                    items.last().map(|item| &item.payload),
                    Some(ItemPayload::Terminal(TerminalOutcome::Cancelled))
                )
            })
    }
}

#[derive(Clone)]
struct BackpressuredProvider;

impl ModelProvider for BackpressuredProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        let mut delta_count = 0;
        Ok(Box::new(std::iter::from_fn(move || {
            if delta_count < 64 {
                delta_count += 1;
                Some(ProviderEvent::Delta("A".to_owned()))
            } else {
                thread::sleep(Duration::from_millis(1));
                Some(ProviderEvent::Pending)
            }
        })))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unread_sse_backpressure_does_not_block_interrupt_terminalization() {
    let history = ConcurrentHistory::default();
    let router = build_router(
        TurnRunner::new(BackpressuredProvider, history.clone()),
        ApprovalsUnavailable,
    );
    let response = router
        .clone()
        .oneshot(chat_request("/api/v1/ai/chat/stream"))
        .await
        .expect("stream response");
    let turn_id = history.accepted_turn_id();

    timeout(Duration::from_millis(500), async {
        while history.item_count(turn_id) < 65 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("all 64 provider deltas become durable without reading the SSE body");

    let interrupt = router
        .oneshot(interrupt_request(turn_id))
        .await
        .expect("interrupt response");
    assert_eq!(interrupt.status(), StatusCode::ACCEPTED);

    let terminalized = timeout(Duration::from_millis(250), async {
        while !history.has_interrupted_terminal(turn_id) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    if terminalized.is_err() {
        drop(response);
        panic!("an unread SSE body blocked the accepted interrupt terminal");
    }

    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("bounded stream body");
    assert!(String::from_utf8_lossy(&body).contains("event: turn.interrupted"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_response_starts_before_provider_completion() {
    let provider = GatedProvider {
        gate: Arc::new((Mutex::new(false), Condvar::new())),
    };
    let router = build_router(
        TurnRunner::new(provider.clone(), ConcurrentHistory::default()),
        ApprovalsUnavailable,
    );
    let mut response_task = tokio::spawn(router.oneshot(chat_request("/api/v1/ai/chat/stream")));

    let response = if let Ok(joined) = timeout(Duration::from_millis(250), &mut response_task).await
    {
        joined
            .expect("response task joins")
            .expect("stream response")
    } else {
        provider.release();
        let _ = response_task.await;
        panic!("stream response waited for provider completion");
    };
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let first = timeout(Duration::from_millis(250), body.next())
        .await
        .expect("turn.started arrives before provider completion")
        .expect("stream has a first chunk")
        .expect("first chunk is readable");
    assert!(String::from_utf8_lossy(&first).contains("event: turn.started"));

    provider.release();
    let mut remainder = String::new();
    while let Some(chunk) = body.next().await {
        remainder.push_str(&String::from_utf8_lossy(&chunk.expect("stream chunk")));
    }
    assert!(remainder.contains("event: item.created"));
    assert!(remainder.contains("event: turn.completed"));
}

struct IdleUpstream {
    base_url: String,
    gate: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl IdleUpstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind idle upstream");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("upstream address")
        );
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let server_gate = Arc::clone(&gate);
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read provider request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .expect("write provider headers");
            stream.flush().expect("flush provider headers");
            let (lock, ready) = &*server_gate;
            let mut released = lock.lock().expect("upstream gate lock");
            while !*released {
                released = ready.wait(released).expect("upstream gate wait");
            }
            stream
                .write_all(b"0\r\n\r\n")
                .expect("finish provider body");
        });
        Self {
            base_url,
            gate,
            thread: Some(thread),
        }
    }

    fn release(&self) {
        let (lock, ready) = &*self.gate;
        *lock.lock().expect("upstream gate lock") = true;
        ready.notify_all();
    }
}

impl Drop for IdleUpstream {
    fn drop(&mut self) {
        self.release();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_is_observed_while_the_provider_stream_is_idle() {
    let upstream = IdleUpstream::start();
    let transport = ReqwestOpenAiTransport::new(
        reqwest::Client::new(),
        tokio::runtime::Handle::current(),
        &upstream.base_url,
        "test-model",
        "test-key",
    );
    let history = ConcurrentHistory::default();
    let router = build_router(
        TurnRunner::new(OpenAiCompatibleProvider::new(transport), history.clone()),
        ApprovalsUnavailable,
    );
    let response = router
        .clone()
        .oneshot(chat_request("/api/v1/ai/chat/stream"))
        .await
        .expect("stream response");
    let mut body = response.into_body().into_data_stream();
    let started = body
        .next()
        .await
        .expect("turn.started chunk")
        .expect("turn.started is readable");
    assert!(String::from_utf8_lossy(&started).contains("event: turn.started"));

    let interrupt = router
        .oneshot(interrupt_request(history.accepted_turn_id()))
        .await
        .expect("interrupt response");
    assert_eq!(interrupt.status(), StatusCode::ACCEPTED);

    let interrupted = timeout(Duration::from_millis(500), body.next()).await;
    let Ok(Some(Ok(interrupted))) = interrupted else {
        upstream.release();
        panic!("idle provider prevented the accepted interrupt from terminalizing");
    };
    assert!(String::from_utf8_lossy(&interrupted).contains("event: turn.interrupted"));
    upstream.release();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_an_idle_sse_body_cancels_the_durable_turn() {
    let upstream = IdleUpstream::start();
    let transport = ReqwestOpenAiTransport::new(
        reqwest::Client::new(),
        tokio::runtime::Handle::current(),
        &upstream.base_url,
        "test-model",
        "test-key",
    );
    let history = ConcurrentHistory::default();
    let router = build_router(
        TurnRunner::new(OpenAiCompatibleProvider::new(transport), history.clone()),
        ApprovalsUnavailable,
    );
    let response = router
        .oneshot(chat_request("/api/v1/ai/chat/stream"))
        .await
        .expect("stream response");
    let turn_id = history.accepted_turn_id();
    let mut body = response.into_body().into_data_stream();
    let started = body
        .next()
        .await
        .expect("turn.started chunk")
        .expect("turn.started is readable");
    assert!(String::from_utf8_lossy(&started).contains("event: turn.started"));

    drop(body);

    timeout(Duration::from_millis(500), async {
        while !history.has_cancelled_terminal(turn_id) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("SSE disconnect durably cancels the idle turn");
    upstream.release();
}

fn completed_result() -> TurnResult {
    let usage = Usage::new(1, 1).expect("valid usage");
    let terminal = Item::new(
        2,
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
    );
    TurnResult {
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        status: TurnStatus::Completed,
        published: vec![terminal.clone()],
        replay: vec![terminal],
    }
}

fn chat_request(path: &str) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .header("x-koduck-tenant-id", "tenant-a")
        .header("x-koduck-subject-id", "subject-a")
        .body(Body::from(r#"{"input":"hello"}"#))
        .expect("valid chat request")
}

fn interrupt_request(turn_id: TurnId) -> Request<Body> {
    Request::post(format!("/api/v1/ai/turns/{}/interrupt", turn_id.as_uuid()))
        .header("x-koduck-tenant-id", "tenant-a")
        .header("x-koduck-subject-id", "subject-a")
        .body(Body::empty())
        .expect("valid interrupt request")
}
