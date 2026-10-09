// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! AC-7: trust rejection and context errors retain the exact v1 problem wire
//! format without leaking source values. Missing identity returns 401 before
//! any history access; foreign tenant/subject/Thread and unknown Thread are
//! indistinguishable 404s; stored decode and projection failures return 503
//! before any SSE stream; problem bodies carry exactly the contract keys and
//! no sentinel reaches external or internal diagnostics.

use koduck_ai::adapters::history::postgres::{
    PostgresExecutor, PostgresTurnHistory, SqlxPostgresExecutor,
};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest, HttpResponse};
use koduck_ai::application::{
    NewItem, ProjectionError, ProviderContextError, TurnCommand, TurnRunError, TurnRunner,
};
use koduck_ai::domain::{TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId, Usage};

use super::support::*;
use crate::fixtures::*;

const SENTINEL_TENANT: &str = "SENTINEL-TENANT-07";
const SENTINEL_SUBJECT: &str = "SENTINEL-SUBJECT-08";
const SENTINEL_CONTENT: &str = "SENTINEL-CONTENT-09";
const SENTINEL_REPLACEMENT: &str = "SENTINEL-REPLACEMENT-10";
const SENTINELS: [&str; 4] = [
    SENTINEL_TENANT,
    SENTINEL_SUBJECT,
    SENTINEL_CONTENT,
    SENTINEL_REPLACEMENT,
];

/// Runs the complete AC-7 check.
pub(crate) fn run() {
    missing_identity_rejects_before_history();
    let (pool, runtime) = migrated_pool();
    let executor = SqlxPostgresExecutor::new(pool.clone(), runtime.handle().clone());
    foreign_and_unknown_threads_are_indistinguishable(&runtime, &pool, &executor);
    decode_and_projection_failures_map_to_503_with_safe_diagnostics(&runtime, &pool, &executor);
    runtime.block_on(pool.close());
}

/// Builds one chat request with an optional identity.
fn request(path: &str, thread: ThreadId, trust: Option<TrustContext>) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Post,
        path: path.to_owned(),
        content_type: Some("application/json".to_owned()),
        body: format!(r#"{{"input":"hello","thread_id":"{}"}}"#, thread.as_uuid()),
        trust,
    }
}

/// Asserts one problem body carries exactly the contract keys and its
/// status/code pair matches, then returns the parsed document.
fn assert_problem_contract(response: &HttpResponse, status: u16, code: &str) -> serde_json::Value {
    assert_eq!(response.status, status);
    assert_eq!(
        response.header("Content-Type"),
        Some("application/problem+json"),
        "problems use the problem JSON media type"
    );
    let document: serde_json::Value = serde_json::from_str(&response.body).expect("JSON body");
    let keys = document
        .as_object()
        .expect("problem object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            "code".to_owned(),
            "correlation_id".to_owned(),
            "status".to_owned(),
            "title".to_owned(),
            "type".to_owned()
        ],
        "the body retains exactly the v1 problem fields"
    );
    assert_eq!(document["status"], serde_json::json!(status));
    assert_eq!(document["code"], serde_json::json!(code));
    assert!(
        document["correlation_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "a fresh correlation id is present"
    );
    document
}

/// Missing identity returns 401 with the Bearer challenge before any
/// history access on both routes.
fn missing_identity_rejects_before_history() {
    let provider = RecordingProvider::new();
    let history = MemoryHistory::default();
    let mut adapter = HttpAdapter::new(TurnRunner::new(provider.clone(), history.clone()));
    for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
        let response = adapter.handle(request(path, ThreadId::new(), None));
        assert_eq!(response.status, 401, "path {path}");
        assert_eq!(response.header("WWW-Authenticate"), Some("Bearer"));
        assert_problem_contract(&response, 401, "invalid-identity");
    }
    assert_eq!(history.prior_reads(), 0, "history is never reached");
    assert!(provider.recorded().is_empty());
}

/// Foreign tenant, foreign subject, and unknown Thread produce the same
/// indistinguishable 404 problem on both routes, with no sentinel leakage.
fn foreign_and_unknown_threads_are_indistinguishable(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let accepted = executor
        .accept_initial(
            &TurnCommand::new(owner.clone(), None, SENTINEL_CONTENT).expect("valid command"),
        )
        .expect("seed an owned Thread");
    executor
        .append(
            &accepted,
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(1, 1).expect("valid usage"),
            }),
        )
        .expect("seed the terminal");
    let foreign_subject = TrustContext::new(
        TenantId::new(SENTINEL_TENANT).expect("valid tenant"),
        SENTINEL_SUBJECT,
    )
    .expect("foreign trust");
    let foreign_tenant = TrustContext::new(
        TenantId::new(format!("foreign-{SENTINEL_TENANT}")).expect("valid tenant"),
        &subject,
    )
    .expect("foreign tenant trust");
    let provider = RecordingProvider::new();
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        provider.clone(),
        PostgresTurnHistory::new(executor.clone()),
    ));

    let cases = [
        (foreign_subject, accepted.thread_id),
        (foreign_tenant, accepted.thread_id),
        (owner.clone(), ThreadId::new()),
    ];
    let mut bodies = Vec::new();
    for (trust, thread) in cases {
        for path in ["/api/v1/ai/chat", "/api/v1/ai/chat/stream"] {
            let response = adapter.handle(request(path, thread, Some(trust.clone())));
            let document = assert_problem_contract(&response, 404, "not-found");
            bodies.push(document);
        }
    }
    let shape = |document: &serde_json::Value| {
        (
            document["type"].clone(),
            document["title"].clone(),
            document["status"].clone(),
            document["code"].clone(),
        )
    };
    assert!(
        bodies
            .iter()
            .all(|document| shape(document) == shape(&bodies[0])),
        "unknown and non-owned Threads are indistinguishable"
    );
    assert!(
        provider.recorded().is_empty(),
        "no provider request is made"
    );
    assert_eq!(
        sql_turn_count(runtime, pool, &tenant, accepted.thread_id),
        1,
        "no new Turn is accepted for foreign or unknown Threads"
    );
}

/// A stored decode failure through real `SQL` and a projection failure
/// through the double both map to 503 before any SSE stream, with redacted
/// internal and external diagnostics and zero effects.
fn decode_and_projection_failures_map_to_503_with_safe_diagnostics(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    executor: &SqlxPostgresExecutor,
) {
    let (tenant, subject) = unique_owner();
    let owner = owner_trust(&tenant, &subject);
    let corrupt_thread = seed_corrupt_thread(runtime, pool, &tenant, &subject);
    let provider = RecordingProvider::new();
    let mut adapter = HttpAdapter::new(TurnRunner::new(
        provider.clone(),
        PostgresTurnHistory::new(executor.clone()),
    ));

    let turns_before = sql_turn_count(runtime, pool, &tenant, corrupt_thread);
    let mut emitted = Vec::new();
    let response = adapter.handle_stream_controlled(
        request(
            "/api/v1/ai/chat/stream",
            corrupt_thread,
            Some(owner.clone()),
        ),
        &mut |event| emitted.push(event),
        &|| false,
    );
    assert_problem_contract(&response, 503, "durability-unavailable");
    assert!(emitted.is_empty(), "the SSE route starts no stream");
    let response = adapter.handle(request(
        "/api/v1/ai/chat",
        corrupt_thread,
        Some(owner.clone()),
    ));
    assert_problem_contract(&response, 503, "durability-unavailable");
    assert_no_sentinels(&response.body);
    assert_eq!(
        sql_turn_count(runtime, pool, &tenant, corrupt_thread),
        turns_before,
        "no new Turn is accepted"
    );
    assert!(
        provider.recorded().is_empty(),
        "no provider request is made"
    );

    internal_projection_diagnostics_redact_sentinels(corrupt_thread);
}

/// The internal typed context error renders no payload or identity sentinel.
fn internal_projection_diagnostics_redact_sentinels(thread: ThreadId) {
    let history = MemoryHistory::default();
    let late = delta_item(2, SENTINEL_CONTENT);
    history.seed_turn(
        thread,
        TurnId::new(),
        vec![correction_item(1, late.item_id, SENTINEL_REPLACEMENT), late],
    );
    let mut runner = TurnRunner::new(RecordingProvider::new(), history);
    let error = runner
        .execute(
            TurnCommand::new(
                TrustContext::new(
                    TenantId::new(SENTINEL_TENANT).expect("valid tenant"),
                    SENTINEL_SUBJECT,
                )
                .expect("sentinel trust"),
                Some(thread),
                SENTINEL_CONTENT,
            )
            .expect("valid command"),
        )
        .expect_err("the corrupt projection rejects");
    assert!(matches!(
        error,
        TurnRunError::Context(ProviderContextError::Projection(
            ProjectionError::ForwardReference
        ))
    ));
    for rendering in [format!("{error}"), format!("{error:?}")] {
        for sentinel in SENTINELS {
            assert!(
                !rendering.contains(sentinel),
                "internal diagnostics redact {sentinel}"
            );
        }
    }
}

/// Seeds one owned Thread holding an undecodable payload row.
fn seed_corrupt_thread(
    runtime: &tokio::runtime::Runtime,
    pool: &sqlx::postgres::PgPool,
    tenant: &TenantId,
    subject: &str,
) -> ThreadId {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    seed_thread_row(runtime, pool, &owner_trust(tenant, subject), thread_id);
    seed_turn_row(
        runtime,
        pool,
        tenant,
        thread_id,
        turn_id,
        "2026-01-01T00:00:00+00:00",
        "completed",
    );
    runtime
        .block_on(
            sqlx::query(
                "INSERT INTO turn_items (tenant_id, thread_id, turn_id, sequence, item_id, \
                 item_type, payload) VALUES ($1, $2, $3, 1, $4, 'agent_message_delta', \
                 'not-json')",
            )
            .bind(tenant.as_str())
            .bind(thread_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(uuid::Uuid::new_v4())
            .execute(pool),
        )
        .expect("seed the undecodable row");
    thread_id
}

/// Asserts no sentinel value appears in one rendered surface.
fn assert_no_sentinels(text: &str) {
    for sentinel in SENTINELS {
        assert!(
            !text.contains(sentinel),
            "external diagnostics redact {sentinel}"
        );
    }
}
