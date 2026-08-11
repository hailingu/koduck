// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::BTreeMap;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::history::postgres::{PostgresExecutor, SqlxPostgresExecutor};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::adapters::provider::{OpenAiProtocolTransport, ReqwestOpenAiTransport};
use koduck_ai::application::{TurnCommand, TurnResult};
use koduck_ai::domain::{
    Item, ItemPayload, TerminalOutcome, TrustContext, TurnId, TurnStatus, Usage,
};
use koduck_ai::runtime::{RuntimeConfig, build_router};
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
