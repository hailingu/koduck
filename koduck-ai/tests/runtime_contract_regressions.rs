// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use std::collections::{BTreeMap, BTreeSet};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::application::{TurnCommand, TurnResult};
use koduck_ai::domain::{TrustContext, TurnId};
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
fn runtime_config_rejects_non_https_provider_url() {
    let mut environment = complete_environment();
    environment.insert(
        "KODUCK_AI_OPENAI_BASE_URL".to_owned(),
        "http://provider.example/v1".to_owned(),
    );

    let error = RuntimeConfig::from_environment(&environment)
        .expect_err("provider credentials require HTTPS");

    assert_eq!(error.to_string(), "invalid KODUCK_AI_OPENAI_BASE_URL");
}

#[derive(Clone)]
struct PanickingService;

impl TurnService for PanickingService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        panic!("injected blocking handler failure");
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_failure_problem_has_exact_fields_and_correlation_id() {
    let response = build_router(PanickingService)
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

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 16_384)
        .await
        .expect("bounded problem body");
    let serde_json::Value::Object(problem) =
        serde_json::from_slice(&body).expect("problem body is JSON")
    else {
        panic!("problem body must be an object");
    };
    assert_eq!(
        problem.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["type", "title", "status", "code", "correlation_id",])
    );
    uuid::Uuid::parse_str(
        problem["correlation_id"]
            .as_str()
            .expect("correlation ID is a string"),
    )
    .expect("correlation ID is a UUID");
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_authenticated_body_uses_owned_invalid_request_problem() {
    let response = build_router(PanickingService)
        .oneshot(
            Request::post("/api/v1/ai/chat")
                .header("content-type", "application/json")
                .header("x-koduck-tenant-id", "tenant-a")
                .header("x-koduck-subject-id", "subject-a")
                .body(Body::from(vec![b'x'; 3_000_000]))
                .expect("valid oversized request envelope"),
        )
        .await
        .expect("router response");

    assert_owned_problem(response, StatusCode::BAD_REQUEST, "invalid-request").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_method_uses_owned_method_not_allowed_problem() {
    let response = build_router(PanickingService)
        .oneshot(
            Request::get("/api/v1/ai/chat")
                .header("x-koduck-tenant-id", "tenant-a")
                .header("x-koduck-subject-id", "subject-a")
                .body(Body::empty())
                .expect("valid unsupported-method request"),
        )
        .await
        .expect("router response");

    assert_owned_problem(
        response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method-not-allowed",
    )
    .await;
}

async fn assert_owned_problem(
    response: axum::response::Response,
    expected_status: StatusCode,
    expected_code: &str,
) {
    assert_eq!(response.status(), expected_status);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    let body = to_bytes(response.into_body(), 16_384)
        .await
        .expect("bounded problem body");
    let serde_json::Value::Object(problem) =
        serde_json::from_slice(&body).expect("problem body is JSON")
    else {
        panic!("problem body must be an object");
    };
    assert_eq!(
        problem.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["type", "title", "status", "code", "correlation_id",])
    );
    assert_eq!(problem["type"], "about:blank");
    assert_eq!(problem["status"], u64::from(expected_status.as_u16()));
    assert_eq!(problem["code"], expected_code);
    uuid::Uuid::parse_str(
        problem["correlation_id"]
            .as_str()
            .expect("correlation ID is a string"),
    )
    .expect("correlation ID is a UUID");
}
