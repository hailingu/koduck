// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! CAND-18 delivery cases on the real Axum router backed by the production
//! runner and migrated PostgreSQL: while the original creator's provider
//! stream is in flight, an exact retry returns its 202 receipt immediately —
//! even when that retry's response is dropped unconsumed — and dropping the
//! original's SSE body drives only the original owner's own durable
//! cancellation: one terminal, the binding retained, and the identical
//! receipt on the final retry (SI-04/SI-07e/SI-07f).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use uuid::Uuid;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use koduck_ai::adapters::history::postgres::PostgresTurnHistory;
use koduck_ai::runtime::build_router;
use tower::ServiceExt;

use super::super::harness::Harness;
use super::locks::{pacing_provider, wait_until_entered};
use super::{connected_history, count_rows};

/// No approval transport is configured for the chat-only router.
#[derive(Clone)]
struct UnconfiguredApprovals;

impl koduck_ai::adapters::http::approvals::ApprovalDecisionTransport for UnconfiguredApprovals {
    fn decide(
        &mut self,
        _trust: &koduck_ai::domain::TrustContext,
        _thread_id: koduck_ai::domain::ThreadId,
        _approval_id: koduck_ai::domain::execution::ApprovalId,
        _decision: koduck_ai::domain::execution::ApprovalDecision,
        _decided_at_millis: u64,
    ) -> koduck_ai::application::ApprovalDecisionOutcome {
        koduck_ai::application::ApprovalDecisionOutcome::Unavailable
    }
}

fn stream_request(tenant: &str, submission: Uuid, input: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/ai/chat/stream")
        .header("content-type", "application/json")
        .header("x-koduck-tenant-id", tenant)
        .header("x-koduck-subject-id", "subject-a")
        .body(Body::from(format!(
            r#"{{"input":"{input}","submission_id":"{submission}"}}"#
        )))
        .expect("request builds")
}

/// SI-07f/SI-04 (ADR-0018): a disconnected retry never cancels or mutates the
/// original owner, and the original creator's own dropped stream closes only
/// its own Turn through the existing durable cancellation path.
pub(crate) fn cand_18_stream_body_drop_isolates_owner() {
    let _database_guard = super::serialize_database_tests();
    let (harness, _history, _unused) = connected_history();
    let tenant = format!("cand18-delivery-{}", Uuid::new_v4());
    let submission = Uuid::new_v4();

    let (provider, _release, entered, original_inputs) = pacing_provider();
    let runner = koduck_ai::runtime::compose_production_runner(
        provider,
        PostgresTurnHistory::new(harness.executor()),
        koduck_ai::application::NoToolExecution,
    );
    let app = build_router(runner, UnconfiguredApprovals);
    let original_response =
        original_stream_with_disconnected_retry(&harness, &app, &tenant, submission, &entered);
    dropped_stream_closes_only_its_owner(
        &harness,
        &app,
        &tenant,
        submission,
        original_response,
        &original_inputs,
    );
}

/// Starts the original creator's SSE stream and completes one exact retry
/// whose response is dropped unconsumed, proving the owner keeps executing
/// with its durable state untouched (SI-07f). Returns the still-open original
/// response for the disconnect half.
fn original_stream_with_disconnected_retry(
    harness: &Harness,
    app: &axum::Router,
    tenant: &str,
    submission: Uuid,
    entered: &Arc<AtomicBool>,
) -> Response {
    let original_response = harness.runtime.block_on(async {
        app.clone()
            .oneshot(stream_request(tenant, submission, "delivered input"))
            .await
            .expect("the original stream responds")
    });
    wait_until_entered(entered);
    assert_eq!(original_response.status(), StatusCode::OK);
    assert_eq!(
        original_response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().expect("ascii")),
        Some("text/event-stream")
    );

    // The exact retry is decided before any SSE with its 202 receipt; dropping
    // the response unconsumed — the disconnected-retry shape — completes
    // without touching the owner.
    let retry_response = harness.runtime.block_on(async {
        app.clone()
            .oneshot(stream_request(tenant, submission, "delivered input"))
            .await
            .expect("the retry responds")
    });
    assert_eq!(retry_response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        retry_response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().expect("ascii")),
        Some("application/json"),
        "the retry is decided before any SSE header"
    );
    drop(retry_response);
    assert!(
        entered.load(Ordering::Acquire),
        "the original owner keeps executing after the retry disconnect"
    );
    let midflight = harness.runtime.block_on(async {
        sqlx::query_as::<_, (String, i64, bool)>(
            "SELECT t.status, l.generation, l.fenced FROM turns t \
             JOIN turn_leases l USING (tenant_id, thread_id, turn_id) \
             JOIN chat_submissions s USING (tenant_id, thread_id, turn_id) \
             WHERE s.tenant_id = $1 AND s.submission_id = $2",
        )
        .bind(tenant)
        .bind(submission)
        .fetch_one(&harness.pool)
        .await
        .expect("read the mid-flight owner state")
    });
    assert_eq!(
        midflight,
        ("started".to_owned(), 1, false),
        "the disconnected retry mutated no owner state"
    );
    original_response
}

/// Drops the original's SSE body — the creator-side disconnect — and proves
/// the runner's own cancellation path closes only its own Turn: one terminal,
/// the binding retained, one provider invocation, and the identical receipt
/// on the final retry (SI-04/SI-07e).
fn dropped_stream_closes_only_its_owner(
    harness: &Harness,
    app: &axum::Router,
    tenant: &str,
    submission: Uuid,
    original_response: Response,
    original_inputs: &Arc<std::sync::Mutex<Vec<String>>>,
) {
    drop(original_response);
    harness.runtime.block_on(async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT t.status FROM turns t JOIN chat_submissions s \
                 USING (tenant_id, thread_id, turn_id) \
                 WHERE s.tenant_id = $1 AND s.submission_id = $2",
            )
            .bind(tenant)
            .bind(submission)
            .fetch_optional(&harness.pool)
            .await
            .expect("poll the cancelled status");
            if status.as_deref() == Some("cancelled") {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the dropped stream never closed its turn: {status:?}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });

    let rows = harness
        .runtime
        .block_on(count_rows(&harness.pool, tenant, "subject-a", submission));
    assert_eq!(rows, (1, 1, 1, 1), "the disconnect retains the acceptance");
    let terminals: i64 = harness.runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT count(*) FROM turn_items i JOIN chat_submissions s \
             USING (tenant_id, thread_id, turn_id) \
             WHERE s.tenant_id = $1 AND s.submission_id = $2 AND i.is_terminal",
        )
        .bind(tenant)
        .bind(submission)
        .fetch_one(&harness.pool)
        .await
        .expect("count terminals")
    });
    assert_eq!(terminals, 1, "exactly one durable terminal closes the turn");
    assert_eq!(
        original_inputs.lock().expect("inputs").as_slice(),
        ["delivered input"]
    );

    let final_response = harness.runtime.block_on(async {
        app.clone()
            .oneshot(stream_request(tenant, submission, "delivered input"))
            .await
            .expect("the final retry responds")
    });
    assert_eq!(final_response.status(), StatusCode::ACCEPTED);
    let final_body = String::from_utf8(
        harness
            .runtime
            .block_on(async {
                to_bytes(final_response.into_body(), 1_048_576)
                    .await
                    .expect("the final body reads")
            })
            .to_vec(),
    )
    .expect("utf-8");
    assert!(
        final_body.contains(r#""status":"accepted""#),
        "the cancelled turn's retry still returns the acceptance receipt: {final_body}"
    );
}
