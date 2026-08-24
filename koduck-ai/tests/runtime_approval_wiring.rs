// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Black-box runtime wiring harness for the approval-decision v1 route.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use koduck_ai::adapters::http::{ServiceError, TurnService};
use koduck_ai::application::{
    ApprovalDecisionResolution, ApprovalDecisionRoute, ApprovalInsertResolution,
    ApprovalRecordStore, ApprovalStoreError, TurnResult,
};
use koduck_ai::domain::execution::{ApprovalDecision, ApprovalId, ApprovalStatus};
use koduck_ai::domain::{TenantId, ThreadId, TrustContext, TurnId, TurnStatus};
use koduck_ai::runtime::build_router;
use tower::ServiceExt;

/// In-memory canonical D-6 double with the store's conditional semantics.
#[derive(Clone, Default)]
struct RecordingApprovals {
    state: Arc<Mutex<ApprovalsState>>,
}

#[derive(Default)]
struct ApprovalsState {
    rows: HashMap<(String, ApprovalId), ApprovalRow>,
    mutations: usize,
}

struct ApprovalRow {
    thread_id: ThreadId,
    requester_subject: String,
    expires_at_millis: u64,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
}

impl RecordingApprovals {
    /// Seeds one requested approval owned by the fixture Thread and subject.
    fn seed_requested(&self, approval_id: ApprovalId, thread_id: ThreadId, requester: &str) {
        self.state
            .lock()
            .expect("approvals state lock")
            .rows
            .insert(
                ("tenant-a".to_owned(), approval_id),
                ApprovalRow {
                    thread_id,
                    requester_subject: requester.to_owned(),
                    // The runtime adapter supplies the production wall clock,
                    // so the seeded window must never expire inside a test run.
                    expires_at_millis: u64::MAX,
                    status: ApprovalStatus::Requested,
                    decision: None,
                    version: 1,
                },
            );
    }

    fn mutations(&self) -> usize {
        self.state.lock().expect("approvals state lock").mutations
    }
}

impl ApprovalRecordStore for RecordingApprovals {
    fn insert_requested(
        &mut self,
        request: &koduck_ai::domain::execution::ApprovalRequest,
        requester_subject: &str,
    ) -> Result<ApprovalInsertResolution, ApprovalStoreError> {
        let mut state = self.state.lock().expect("approvals state lock");
        let key = (
            request.tenant_id().as_str().to_owned(),
            request.approval_id(),
        );
        if let Some(row) = state.rows.get(&key) {
            return Ok(ApprovalInsertResolution::Existing {
                status: row.status,
                decision: row.decision,
                version: row.version,
            });
        }
        state.rows.insert(
            key,
            ApprovalRow {
                thread_id: request.binding().thread_id(),
                requester_subject: requester_subject.to_owned(),
                expires_at_millis: request.expires_at_millis(),
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
        );
        Ok(ApprovalInsertResolution::Inserted)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "ownership dimensions are individually conditional lookup keys"
    )]
    fn resolve_decision(
        &mut self,
        approval_id: ApprovalId,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        requester_subject: &str,
        decision: ApprovalDecision,
        _approver: &koduck_ai::domain::execution::ApproverId,
        decided_at_millis: u64,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
        let mut state = self.state.lock().expect("approvals state lock");
        let key = (tenant_id.as_str().to_owned(), approval_id);
        let Some(row) = state.rows.get(&key) else {
            return Ok(ApprovalDecisionResolution::NotFound);
        };
        if row.requester_subject != requester_subject || row.thread_id != thread_id {
            return Ok(ApprovalDecisionResolution::NotFound);
        }
        if row.status != ApprovalStatus::Requested {
            return Ok(ApprovalDecisionResolution::ExistingTerminal {
                decision: row.decision,
                status: row.status,
                version: row.version,
            });
        }
        let expires_at_millis = row.expires_at_millis;
        state.mutations += 1;
        let row = state
            .rows
            .get_mut(&key)
            .expect("row remains present under the held lock");
        if decided_at_millis >= expires_at_millis {
            row.status = ApprovalStatus::Expired;
            row.version += 1;
            return Ok(ApprovalDecisionResolution::ExistingTerminal {
                decision: None,
                status: ApprovalStatus::Expired,
                version: row.version,
            });
        }
        row.status = match decision {
            ApprovalDecision::Accepted => ApprovalStatus::Accepted,
            ApprovalDecision::Declined => ApprovalStatus::Declined,
            ApprovalDecision::Cancelled => ApprovalStatus::Cancelled,
        };
        row.decision = Some(decision);
        row.version += 1;
        Ok(ApprovalDecisionResolution::Won {
            decision,
            version: row.version,
        })
    }
}

#[derive(Clone)]
struct StubTurns;

impl TurnService for StubTurns {
    fn execute(
        &mut self,
        _command: koduck_ai::application::TurnCommand,
    ) -> Result<TurnResult, ServiceError> {
        Ok(TurnResult {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            status: TurnStatus::Completed,
            published: Vec::new(),
            replay: Vec::new(),
        })
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

fn seeded_router() -> (axum::Router, RecordingApprovals, ApprovalId, ThreadId) {
    let store = RecordingApprovals::default();
    let approval_id = ApprovalId::new();
    let thread = ThreadId::new();
    store.seed_requested(approval_id, thread, "requester");
    let router = build_router(StubTurns, ApprovalDecisionRoute::new(store.clone()));
    (router, store, approval_id, thread)
}

fn decision_builder(
    approval_id: ApprovalId,
    thread_raw: Option<String>,
    content_type: &str,
) -> axum::http::request::Builder {
    unscoped_decision_builder(approval_id, thread_raw)
        .header("content-type", content_type)
        .header("x-koduck-approval-scopes", "ai.tool.approve")
}

fn unscoped_decision_builder(
    approval_id: ApprovalId,
    thread_raw: Option<String>,
) -> axum::http::request::Builder {
    let builder = Request::post(format!(
        "/api/v1/ai/approvals/{}/decisions",
        approval_id.as_uuid()
    ))
    .header("x-koduck-tenant-id", "tenant-a")
    .header("x-koduck-subject-id", "requester");
    match thread_raw {
        Some(raw) => builder.header("x-koduck-thread-id", raw),
        None => builder,
    }
}

fn decision_request(approval_id: ApprovalId, thread: Option<ThreadId>) -> Request<Body> {
    decision_builder(
        approval_id,
        thread.map(|thread| thread.as_uuid().to_string()),
        "application/json",
    )
    .body(Body::from(r#"{"decision":"accepted"}"#))
    .expect("valid decision request")
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("bounded response body");
    String::from_utf8(bytes.to_vec()).expect("response body is UTF-8")
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_route_rejects_identity_thread_and_scope_failures_closed() {
    let (router, store, approval_id, thread) = seeded_router();

    // Missing identity is 401 with the owned authenticate challenge.
    let missing_identity = router
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/v1/ai/approvals/{}/decisions",
                approval_id.as_uuid()
            ))
            .header("content-type", "application/json")
            .header("x-koduck-thread-id", thread.as_uuid().to_string())
            .body(Body::from(r#"{"decision":"accepted"}"#))
            .expect("valid request shape"),
        )
        .await
        .expect("router response");
    assert_eq!(missing_identity.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        missing_identity
            .headers()
            .get("www-authenticate")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer")
    );

    // An absent, malformed, or mismatched Thread routing context learns
    // nothing: indistinguishable 404 with zero store mutations.
    for thread_context in [None, Some("not-a-uuid".to_owned())] {
        let request = decision_builder(approval_id, thread_context, "application/json")
            .body(Body::from(r#"{"decision":"accepted"}"#))
            .expect("valid request shape");
        let response = router
            .clone()
            .oneshot(request)
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let wrong_thread = router
        .clone()
        .oneshot(decision_request(approval_id, Some(ThreadId::new())))
        .await
        .expect("router response");
    assert_eq!(wrong_thread.status(), StatusCode::NOT_FOUND);

    // An unscoped principal resolves nothing even with the exact Thread.
    let unscoped = router
        .clone()
        .oneshot(
            unscoped_decision_builder(approval_id, Some(thread.as_uuid().to_string()))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"decision":"accepted"}"#))
                .expect("valid request shape"),
        )
        .await
        .expect("router response");
    assert_eq!(unscoped.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        store.mutations(),
        0,
        "identity, Thread, and scope failures mutate nothing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_route_rejects_duplicate_thread_routing_headers() {
    // A duplicate routing header is ambiguous client context, even if one
    // value matches the requested approval. It must be indistinguishable from
    // an absent or malformed route and cannot authorize a mutation.
    let (router, store, approval_id, thread) = seeded_router();
    let request = decision_builder(
        approval_id,
        Some(thread.as_uuid().to_string()),
        "application/json",
    )
    .header("x-koduck-thread-id", "not-a-uuid")
    .body(Body::from(r#"{"decision":"accepted"}"#))
    .expect("valid request shape");

    let response = router.oneshot(request).await.expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(store.mutations(), 0, "ambiguous routing mutates nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_route_rejects_method_and_content_type_before_decision() {
    let (router, store, approval_id, thread) = seeded_router();

    let wrong_method = router
        .clone()
        .oneshot(
            decision_builder(
                approval_id,
                Some(thread.as_uuid().to_string()),
                "application/json",
            )
            .method(axum::http::Method::GET)
            .body(Body::from(r#"{"decision":"accepted"}"#))
            .expect("valid request shape"),
        )
        .await
        .expect("router response");
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);

    let wrong_content_type = router
        .clone()
        .oneshot(
            decision_builder(
                approval_id,
                Some(thread.as_uuid().to_string()),
                "text/plain",
            )
            .body(Body::from(r#"{"decision":"accepted"}"#))
            .expect("valid request shape"),
        )
        .await
        .expect("router response");
    assert_eq!(wrong_content_type.status(), StatusCode::BAD_REQUEST);
    assert_eq!(store.mutations(), 0, "transport failures mutate nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn approval_route_commits_replays_and_conflicts_through_the_runtime_router() {
    let (router, store, approval_id, thread) = seeded_router();

    // A scoped owning principal through the exact Thread commits the decision
    // and receives the exact canonical terminal projection.
    let valid = router
        .clone()
        .oneshot(decision_request(approval_id, Some(thread)))
        .await
        .expect("router response");
    assert_eq!(valid.status(), StatusCode::OK);
    let terminal = format!(
        "{{\"approval_id\":\"{id}\",\"status\":\"accepted\",\"decision\":\"accepted\",\"version\":2}}",
        id = approval_id.as_uuid()
    );
    assert_eq!(body_text(valid).await, terminal);
    assert_eq!(store.mutations(), 1);

    // An identical replay returns the same terminal projection.
    let replay = router
        .clone()
        .oneshot(decision_request(approval_id, Some(thread)))
        .await
        .expect("router response");
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_text(replay).await, terminal);

    // A conflicting decision is 409 and mutates nothing further.
    let conflict = router
        .clone()
        .oneshot(
            decision_builder(
                approval_id,
                Some(thread.as_uuid().to_string()),
                "application/json",
            )
            .body(Body::from(r#"{"decision":"declined"}"#))
            .expect("valid conflict request"),
        )
        .await
        .expect("router response");
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(store.mutations(), 1, "exactly one mutation across the run");
}
