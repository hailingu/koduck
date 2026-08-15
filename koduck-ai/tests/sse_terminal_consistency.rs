// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::application::{TurnCommand, TurnResult, TurnStreamEvent};
use koduck_ai::domain::{
    Item, ItemPayload, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage,
};
use koduck_ai::runtime::build_router;
use tower::ServiceExt;

/// Turn-only router fixture: no approval transport is configured.
#[derive(Clone)]
struct ApprovalsUnavailable;

impl koduck_ai::adapters::http::approvals::ApprovalDecisionTransport for ApprovalsUnavailable {
    fn decide(
        &mut self,
        _trust: &TrustContext,
        _thread_id: koduck_ai::domain::ThreadId,
        _approval_id: koduck_ai::domain::execution::ApprovalId,
        _decision: koduck_ai::domain::execution::ApprovalDecision,
        _decided_at_millis: u64,
    ) -> koduck_ai::application::ApprovalDecisionOutcome {
        koduck_ai::application::ApprovalDecisionOutcome::Unavailable
    }
}

#[derive(Clone)]
struct ReplayFailureAfterTerminal;

impl TurnService for ReplayFailureAfterTerminal {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        Err(ServiceError::DurabilityUnavailable)
    }

    fn execute_stream(
        &mut self,
        _command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, ServiceError> {
        let thread_id = ThreadId::new();
        let turn_id = TurnId::new();
        observer(TurnStreamEvent::Started { thread_id, turn_id });
        observer(TurnStreamEvent::Item {
            thread_id,
            turn_id,
            item: Item::new(
                2,
                ItemPayload::Terminal(TerminalOutcome::Completed {
                    usage: Usage::zero(),
                }),
            ),
        });
        Err(ServiceError::DurabilityUnavailable)
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_failure_after_sse_terminal_does_not_emit_error_event() {
    let response = build_router(ReplayFailureAfterTerminal, ApprovalsUnavailable)
        .oneshot(
            Request::post("/api/v1/ai/chat/stream")
                .header("content-type", "application/json")
                .header("x-koduck-tenant-id", "tenant-a")
                .header("x-koduck-subject-id", "subject-a")
                .body(Body::from(r#"{"input":"hello"}"#))
                .expect("valid request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 16_384)
        .await
        .expect("bounded stream body");
    let body = String::from_utf8(body.to_vec()).expect("SSE body is UTF-8");
    assert!(body.contains("event: turn.completed"));
    assert!(!body.contains("event: error"));
}
