// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::adapters::provider::{OpenAiProtocolTransport, ReqwestOpenAiTransport};
use koduck_ai::application::{
    AcceptedTurn, HistoryError, ModelInput, ModelProvider, NewItem, ProviderError, ProviderEvent,
    ProviderStream, TurnCommand, TurnHistory, TurnResult, TurnRunner,
};
use koduck_ai::domain::{
    Item, ItemPayload, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
    TurnStatus, Usage,
};
use koduck_ai::runtime::{RuntimeConfig, build_router};
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tower::ServiceExt;

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
    let router = build_router(StubService);
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
    let router = build_router(service.clone());
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
}

#[derive(Clone, Default)]
struct ConcurrentHistory {
    state: Arc<Mutex<ConcurrentHistoryState>>,
}

impl TurnHistory for ConcurrentHistory {
    fn request_interrupt(
        &mut self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        Ok(())
    }

    fn interruption_requested(&self, _turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        Ok(false)
    }

    fn prior_thread_items(
        &self,
        _tenant_id: &TenantId,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_response_starts_before_provider_completion() {
    let provider = GatedProvider {
        gate: Arc::new((Mutex::new(false), Condvar::new())),
    };
    let router = build_router(TurnRunner::new(
        provider.clone(),
        ConcurrentHistory::default(),
    ));
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
