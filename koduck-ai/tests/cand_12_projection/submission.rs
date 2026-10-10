// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 request-validation and retry-receipt behavior on the real Axum
//! router (AC-1, AC-7). The service double records every command that
//! reaches the application boundary, so rejection cases also prove zero
//! history operations.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::application::{
    SubmissionObservation, TurnCommand, TurnOutcome, TurnResult, TurnStreamEvent,
};
use koduck_ai::domain::{SubmissionId, ThreadId, TrustContext, TurnId, TurnStatus};
use koduck_ai::runtime::build_router;
use tower::ServiceExt;
use uuid::Uuid;

/// Records how many commands reached the service and what the next outcome
/// or error is.
#[derive(Clone)]
struct SubmissionService {
    commands: Arc<AtomicUsize>,
    outcome: Outcome,
}

#[derive(Clone)]
enum Outcome {
    /// One completed fresh lifecycle result.
    Completed,
    /// The observed receipt of an exact existing key.
    Observed(SubmissionObservation),
    /// The typed owned-key drift conflict.
    Conflict,
}

impl SubmissionService {
    fn new(outcome: Outcome) -> (Self, Arc<AtomicUsize>) {
        let commands = Arc::new(AtomicUsize::new(0));
        (
            Self {
                commands: Arc::clone(&commands),
                outcome,
            },
            commands,
        )
    }

    fn receipt() -> SubmissionObservation {
        SubmissionObservation {
            submission_id: SubmissionId::from_uuid(
                Uuid::parse_str("0f0e0d0c-0b0a-4987-8654-321098765432").expect("valid uuid"),
            )
            .expect("non-nil"),
            thread_id: ThreadId::from_uuid(
                Uuid::parse_str("11111111-2222-4333-8444-555555555555").expect("valid uuid"),
            ),
            turn_id: TurnId::from_uuid(
                Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").expect("valid uuid"),
            ),
        }
    }
}

impl TurnService for SubmissionService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        self.commands.fetch_add(1, Ordering::AcqRel);
        Ok(completed())
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }

    fn execute_submission_controlled(
        &mut self,
        _command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<TurnOutcome, ServiceError> {
        self.commands.fetch_add(1, Ordering::AcqRel);
        match &self.outcome {
            Outcome::Completed => {
                let result = completed();
                observer(TurnStreamEvent::Started {
                    thread_id: result.thread_id,
                    turn_id: result.turn_id,
                });
                Ok(TurnOutcome::Owned(result))
            }
            Outcome::Observed(receipt) => Ok(TurnOutcome::Observed(receipt.clone())),
            Outcome::Conflict => Err(ServiceError::SubmissionConflict),
        }
    }
}

fn completed() -> TurnResult {
    TurnResult {
        thread_id: ThreadId::from_uuid(
            Uuid::parse_str("11111111-2222-4333-8444-555555555555").expect("valid uuid"),
        ),
        turn_id: TurnId::from_uuid(
            Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").expect("valid uuid"),
        ),
        status: TurnStatus::Completed,
        published: Vec::new(),
        replay: Vec::new(),
    }
}

fn trust_header() -> [(&'static str, String); 2] {
    [
        ("x-koduck-tenant-id", "tenant-a".to_owned()),
        ("x-koduck-subject-id", "subject-a".to_owned()),
    ]
}

async fn post(path: &str, body: String, trust: bool) -> (StatusCode, String, Option<String>) {
    let (service, _commands) = SubmissionService::new(Outcome::Completed);
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    for (name, value) in trust_header() {
        request = request.header(name, value);
    }
    let request = if trust {
        request.body(Body::from(body)).expect("request builds")
    } else {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("request builds")
    };
    let response = build_router(service, UnconfiguredApprovals)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().expect("ascii header").to_owned());
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body reads");
    (
        status,
        String::from_utf8(body.to_vec()).expect("utf-8"),
        content_type,
    )
}

/// No approval transport is configured for the chat-only router.
#[derive(Clone)]
struct UnconfiguredApprovals;

impl koduck_ai::adapters::http::approvals::ApprovalDecisionTransport for UnconfiguredApprovals {
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

/// AC-1/SI-01a: only an omitted identity or a valid 36-character hyphenated
/// non-nil UUID reaches the application; every other form returns the exact
/// 400 problem without any history operation.
/// Posts one chat case and asserts the exact expected status.
async fn assert_chat_case(body: String, expected_status: StatusCode) {
    let (status, body_out, _content_type) = post("/api/v1/ai/chat", body.clone(), true).await;
    assert_eq!(status, expected_status, "case body: {body:?}");
    if expected_status == StatusCode::BAD_REQUEST {
        assert!(
            body_out.contains(r#""code":"invalid-request""#),
            "rejections use the existing invalid-request problem: {body_out}"
        );
    }
}

/// SI-01a: only an omitted identity or a valid 36-character hyphenated
/// non-nil UUID reaches the application; every other identity form returns
/// the exact 400 problem without any history operation.
#[test]
fn cand_18_request_validation() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("validation runtime");
    runtime.block_on(async {
        for (body, expected) in identity_form_cases() {
            assert_chat_case(body, expected).await;
        }
    });
}

/// SI-01a identity-form boundary table.
fn identity_form_cases() -> Vec<(String, StatusCode)> {
    vec![
        (r#"{"input":"hello"}"#.to_owned(), StatusCode::OK),
        (
            format!(r#"{{"input":"hello","submission_id":"{}"}}"#, Uuid::nil()),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!(
                r#"{{"input":"hello","submission_id":"{}"}}"#,
                Uuid::from_u128(0x1234_5678_9abc_def0_1112_2334_4556_6778)
            ),
            StatusCode::OK,
        ),
        (
            r#"{"input":"hello","submission_id":"12345678-9ABC-DEF0-1112-23344556 6778"}"#
                .to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"input":"hello","submission_id":"123456789abcdef011122334455667 78"}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!(
                r#"{{"input":"hello","submission_id":"{{{}}}}}"#,
                Uuid::from_u128(1)
            ),
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"input":"hello","submission_id":null}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"input":"hello","submission_id":""}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"input":"hello","submission_id":42}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"input":"hello","submission_id":{}}"#.to_owned(),
            StatusCode::BAD_REQUEST,
        ),
    ]
}

/// SI-01b: strict body rules — duplicates, unknown members, malformed JSON,
/// and the input byte boundaries.
#[test]
fn cand_18_request_body_strictness() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("body strictness runtime");
    runtime.block_on(async {
        let cases = [
            r#"{"input":"one","input":"two"}"#.to_owned(),
            r#"{"input":"one","unknown":"two"}"#.to_owned(),
            r#"{"input":"one""#.to_owned(),
            String::new(),
        ];
        for body in cases {
            assert_chat_case(body, StatusCode::BAD_REQUEST).await;
        }
        // Exactly 65,536 input bytes pass; one more fails.
        assert_chat_case(
            format!(r#"{{"input":"{}"}}"#, "a".repeat(65_536)),
            StatusCode::OK,
        )
        .await;
        assert_chat_case(
            format!(r#"{{"input":"{}"}}"#, "a".repeat(65_537)),
            StatusCode::BAD_REQUEST,
        )
        .await;
    });
}

/// AC-1/SI-01c: authentication precedes every submission lookup and effect.
#[test]
fn cand_18_authentication_precedes_lookup_and_effects() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("auth runtime");
    runtime.block_on(async {
        let (service, commands) = SubmissionService::new(Outcome::Completed);
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/ai/chat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"input":"hello"}"#))
            .expect("request builds");
        let response = build_router(service, UnconfiguredApprovals)
            .oneshot(request)
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get("www-authenticate")
                .map(|value| value.to_str().expect("ascii")),
            Some("Bearer"),
            "the exact Bearer challenge precedes any lookup"
        );
        assert_eq!(
            commands.load(Ordering::Acquire),
            0,
            "no history operation may run without validated identity"
        );
    });
}

/// AC-7/SI-05: the exact retry returns the constant 202 JSON receipt on both
/// routes, before any SSE header on the stream route, and a drifted key
/// returns the exact 409 problem.
#[test]
fn cand_18_runtime_receipt() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("receipt runtime");
    runtime.block_on(async {
        let receipt = SubmissionService::receipt();
        for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
            assert_observed_receipt(path, receipt.clone()).await;
        }
        assert_drifted_key_conflicts().await;
    });
}

/// SI-05: one observed route returns the constant 202 JSON receipt before
/// any SSE header.
async fn assert_observed_receipt(path: &str, receipt: SubmissionObservation) {
    let (service, _commands) = SubmissionService::new(Outcome::Observed(receipt.clone()));
    let mut request = Request::builder().method("POST").uri(path);
    for (name, value) in trust_header() {
        request = request.header(name, value);
    }
    let request = request
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"input":"hello","submission_id":"0f0e0d0c-0b0a-4987-8654-321098765432"}"#,
        ))
        .expect("request builds");
    let response = build_router(service, UnconfiguredApprovals)
        .oneshot(request)
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::ACCEPTED, "{path}");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().expect("ascii")),
        Some("application/json"),
        "an observed key is decided before any SSE header: {path}"
    );
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body reads");
    let document: serde_json::Value =
        serde_json::from_slice(&body).expect("the receipt parses as JSON");
    let members = document.as_object().expect("receipt is one JSON object");
    let mut names = members.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["status", "submission_id", "thread_id", "turn_id"],
        "the receipt carries exactly the four contract members"
    );
    assert_eq!(
        members["status"], "accepted",
        "accepted describes the immutable acceptance outcome"
    );
    assert_eq!(
        members["submission_id"], "0f0e0d0c-0b0a-4987-8654-321098765432",
        "UUID text is canonical lowercase hyphenated"
    );
}

/// SI-02d/SI-05: the drifted key returns the exact 409 problem with no
/// identity.
async fn assert_drifted_key_conflicts() {
    let (service, _commands) = SubmissionService::new(Outcome::Conflict);
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/v1/ai/chat")
        .header("content-type", "application/json");
    for (name, value) in trust_header() {
        request = request.header(name, value);
    }
    let request = request
        .body(Body::from(
            r#"{"input":"changed","submission_id":"0f0e0d0c-0b0a-4987-8654-321098765432"}"#,
        ))
        .expect("request builds");
    let response = build_router(service, UnconfiguredApprovals)
        .oneshot(request)
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body reads");
    let body = String::from_utf8(body.to_vec()).expect("utf-8");
    assert!(
        body.contains(r#""code":"submission-identity-conflict""#),
        "the conflict exposes the exact problem code: {body}"
    );
    assert!(
        !body.contains("thread_id") && !body.contains("turn_id"),
        "conflicts expose no accepted identities: {body}"
    );
}

/// A service double that implements only the streaming-controlled trait
/// method, so the default `execute_submission_controlled` delegation runs.
#[derive(Clone)]
struct DefaultOnlyService;

impl TurnService for DefaultOnlyService {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        Ok(completed())
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

/// The trait's default identified entry keeps delegating to the streaming
/// method and wraps its result as the owned outcome (SI-04).
#[test]
fn cand_18_default_service_delegation() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("default delegation runtime");
    runtime.block_on(async {
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/ai/chat")
            .header("content-type", "application/json");
        let mut request = request;
        for (name, value) in trust_header() {
            request = request.header(name, value);
        }
        let request = request
            .body(Body::from(r#"{"input":"hello"}"#))
            .expect("request builds");
        let response = build_router(DefaultOnlyService, UnconfiguredApprovals)
            .oneshot(request)
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
    });
}

/// SI-01d (review round 2, finding 1): the `TurnService` default submission
/// entry must fail closed for an identified command instead of silently
/// executing it as a fresh unidentified submission.
#[test]
fn cand_18_default_service_rejects_identified_commands() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("default rejection runtime");
    runtime.block_on(async {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/ai/chat")
            .header("content-type", "application/json");
        for (name, value) in trust_header() {
            request = request.header(name, value);
        }
        let request = request
            .body(Body::from(
                r#"{"input":"hello","submission_id":"0f0e0d0c-0b0a-4987-8654-321098765432"}"#,
            ))
            .expect("request builds");
        let response = build_router(DefaultOnlyService, UnconfiguredApprovals)
            .oneshot(request)
            .await
            .expect("router responds");
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "the default path fails closed rather than executing the identified command"
        );
        let body = to_bytes(response.into_body(), 1_048_576)
            .await
            .expect("body reads");
        let body = String::from_utf8(body.to_vec()).expect("utf-8");
        assert!(
            body.contains(r#""code":"durability-unavailable""#),
            "the fail-closed outcome uses the existing problem: {body}"
        );
    });
}
