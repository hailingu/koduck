// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Production runtime configuration and executable assembly.

pub(crate) mod tool_executor;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::net::{AddrParseError, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::extract::rejection::BytesRejection;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use axum::routing::any;
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;

use crate::adapters::audit::{SerializingToolAuditTrail, SqlxToolAuditSink};
use crate::adapters::execution::{
    SqlxApprovalRecordStore, SqlxExecutionAttemptStore, SqlxTurnLeaseValidator,
};
use crate::adapters::history::postgres::{
    PostgresTurnHistory, SqlxPostgresExecutor, TurnTerminalObserver, unix_time_ms,
};
use crate::adapters::http::{
    HttpAdapter, HttpMethod, HttpRequest, TurnService, approvals::ApprovalDecisionAdapter,
    approvals::ApprovalDecisionTransport, invalid_request_response,
};
use crate::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use crate::application::{AppendPolicy, ApprovalDecisionRoute, TurnRunner};
use crate::domain::{TenantId, ThreadId, TrustContext};

const BIND_ADDR: &str = "KODUCK_AI_BIND_ADDR";
const DATABASE_URL: &str = "KODUCK_AI_DATABASE_URL";
const PROVIDER_BASE_URL: &str = "KODUCK_AI_OPENAI_BASE_URL";
const PROVIDER_MODEL: &str = "KODUCK_AI_OPENAI_MODEL";
const PROVIDER_API_KEY: &str = "KODUCK_AI_OPENAI_API_KEY";
const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Gateway-validated approval-scope header of the trusted context channel.
const APPROVAL_SCOPES_HEADER: &str = "x-koduck-approval-scopes";
/// Thread routing context header for the approval-decision route.
const THREAD_ROUTING_HEADER: &str = "x-koduck-thread-id";
/// Maximum number of validated scopes one principal may carry.
const MAX_APPROVAL_SCOPES: usize = 16;
/// Maximum size of one validated scope token in bytes.
const MAX_APPROVAL_SCOPE_BYTES: usize = 128;
// One turn.started chunk, up to 64 provider items, and one terminal or error chunk.
const STREAM_BUFFER_CAPACITY: usize = 66;

/// Validated process configuration required to assemble the AI runtime.
#[derive(Clone, Eq, PartialEq)]
pub struct RuntimeConfig {
    bind_addr: SocketAddr,
    database_url: String,
    provider_base_url: String,
    provider_model: String,
    provider_api_key: String,
}

impl RuntimeConfig {
    /// Validates the complete runtime environment without reading global state.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeConfigError`] when a required value is absent or the
    /// bind address is not a socket address.
    pub fn from_environment(
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, RuntimeConfigError> {
        let bind_addr = required(environment, BIND_ADDR)?
            .parse()
            .map_err(RuntimeConfigError::InvalidBindAddress)?;
        let provider_base_url = required(environment, PROVIDER_BASE_URL)?;
        let provider_url = reqwest::Url::parse(provider_base_url)
            .map_err(|_| RuntimeConfigError::InvalidProviderBaseUrl)?;
        if provider_url.scheme() != "https"
            || provider_url.host_str().is_none()
            || !provider_url.username().is_empty()
            || provider_url.password().is_some()
            || provider_url.query().is_some()
            || provider_url.fragment().is_some()
        {
            return Err(RuntimeConfigError::InvalidProviderBaseUrl);
        }
        Ok(Self {
            bind_addr,
            database_url: required(environment, DATABASE_URL)?.to_owned(),
            provider_base_url: provider_base_url.to_owned(),
            provider_model: required(environment, PROVIDER_MODEL)?.to_owned(),
            provider_api_key: required(environment, PROVIDER_API_KEY)?.to_owned(),
        })
    }

    /// Reads and validates the current process environment.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeConfigError`] when required runtime configuration is
    /// absent or invalid.
    pub fn from_process_environment() -> Result<Self, RuntimeConfigError> {
        Self::from_environment(&std::env::vars().collect())
    }

    /// Returns the validated server bind address.
    #[must_use]
    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    /// Returns the `PostgreSQL` connection URL.
    #[must_use]
    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Returns the OpenAI-compatible API base URL.
    #[must_use]
    pub fn provider_base_url(&self) -> &str {
        &self.provider_base_url
    }

    /// Returns the configured provider model name.
    #[must_use]
    pub fn provider_model(&self) -> &str {
        &self.provider_model
    }

    /// Returns the provider credential for transport construction.
    #[must_use]
    pub fn provider_api_key(&self) -> &str {
        &self.provider_api_key
    }
}

/// Builds the four owned v1 Axum routes around the framework-neutral adapters.
///
/// The approval-decision router keeps its own state so the turn and approval
/// transports stay independently generic while one builder owns the complete
/// public route set.
pub fn build_router<S, A>(service: S, approvals: A) -> Router
where
    S: TurnService + Clone + Send + Sync + 'static,
    A: ApprovalDecisionTransport + Clone + Send + Sync + 'static,
{
    let turn_state = Arc::new(service);
    let approvals_router = Router::new()
        .route(
            "/api/v1/ai/approvals/{approval_id}/decisions",
            any(handle_approval_request::<A>),
        )
        .with_state(Arc::new(approvals));
    Router::new()
        .route("/api/v1/ai/chat", any(handle_request::<S>))
        .route("/api/v1/ai/chat/stream", any(handle_request::<S>))
        .route(
            "/api/v1/ai/turns/{turn_id}/interrupt",
            any(handle_request::<S>),
        )
        .with_state(turn_state)
        .merge(approvals_router)
}

/// Connects production adapters, applies the owned migration, and serves HTTP.
///
/// # Errors
///
/// Returns [`RuntimeError`] when `PostgreSQL`, provider-client construction,
/// listener binding, or HTTP serving fails.
pub async fn run(config: RuntimeConfig) -> Result<(), RuntimeError> {
    // Assembled at startup and held for the process lifetime: the state owns
    // the process's sole C-5 Turn authority root and the empty-inventory
    // tool-call executor backing the runner's Tool-call servicing.
    let runtime_state = RuntimeState::assemble();
    let database_deadline = AppendPolicy::cand_1().deadline();
    let pool = database_setup_attempt(
        database_deadline,
        PgPoolOptions::new().connect(config.database_url()),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!("../../migrations/0001_cand_1_history.sql")).execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!(
            "../../migrations/0002_cand_2_policy_execution.sql"
        ))
        .execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!(
            "../../migrations/0003_cand_2_requester_ownership.sql"
        ))
        .execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!(
            "../../migrations/0004_cand_2_tool_projections.sql"
        ))
        .execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!(
            "../../migrations/0005_cand_2_execution_attempts.sql"
        ))
        .execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!(
            "../../migrations/0006_cand_2_interrupt_barrier.sql"
        ))
        .execute(&pool),
    )
    .await?;
    database_setup_attempt(
        database_deadline,
        sqlx::raw_sql(include_str!("../../migrations/0007_cand_2_tool_audit.sql")).execute(&pool),
    )
    .await?;
    let runtime = tokio::runtime::Handle::current();
    // Production canonical D-6 assembly: the authenticated decision route
    // drives the conditional `SQLx` transitions on the same Tokio runtime.
    let approvals =
        ApprovalDecisionRoute::new(SqlxApprovalRecordStore::new(pool.clone(), runtime.clone()));
    // Production canonical D-7 assembly: the runner's C-5 boundary commits
    // every terminal through the durable conditional `SQLx` transitions
    // (ADR-0003 TC-12), so the process-local arbitration catalog is no longer
    // the terminal authority. Dispatch and authenticated interruption validate
    // the durable C-6 lease before any D-7 mutation (ADR-0003 TC-07).
    let attempts = SqlxExecutionAttemptStore::new(pool.clone(), runtime.clone());
    let lease = SqlxTurnLeaseValidator::new(pool.clone(), runtime.clone());
    // Production C-5 audit trail: every policy, approval, and execution
    // terminal emits one bounded, correlated record durably appended to the
    // canonical trail (ADR-0003 TC-14).
    let audit_trail =
        SerializingToolAuditTrail::new(SqlxToolAuditSink::new(pool.clone(), runtime.clone()));
    let history = PostgresTurnHistory::new(SqlxPostgresExecutor::new(pool, runtime.clone()))
        .with_terminal_observer(runtime_state.terminal_observer(attempts.clone()));
    let _reconciliation_worker = history
        .start_reconciliation_worker()
        .map_err(RuntimeError::ReconciliationWorker)?;
    let client = reqwest::Client::builder()
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .build()
        .map_err(RuntimeError::ProviderClient)?;
    let transport = ReqwestOpenAiTransport::new(
        client,
        runtime,
        config.provider_base_url(),
        config.provider_model(),
        config.provider_api_key(),
    );
    let provider = OpenAiCompatibleProvider::new(transport);
    let runner = TurnRunner::new(provider, history).with_tool_executor(
        runtime_state.tool_call_executor(attempts.clone(), lease, audit_trail, attempts),
    );
    let listener = tokio::net::TcpListener::bind(config.bind_addr())
        .await
        .map_err(RuntimeError::Bind)?;
    axum::serve(listener, build_router(runner, approvals))
        .await
        .map_err(RuntimeError::Serve)
}

async fn database_setup_attempt<T>(
    deadline: Duration,
    operation: impl Future<Output = Result<T, sqlx::Error>>,
) -> Result<T, RuntimeError> {
    tokio::time::timeout(deadline, operation)
        .await
        .map_err(|_| RuntimeError::DatabaseTimeout)?
        .map_err(RuntimeError::Database)
}

async fn handle_request<S>(
    State(service): State<Arc<S>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response
where
    S: TurnService + Clone + Send + Sync + 'static,
{
    let trust = trust_context(&headers);
    let method = if method == Method::POST {
        HttpMethod::Post
    } else {
        HttpMethod::Other
    };
    let body = if method == HttpMethod::Other {
        String::new()
    } else {
        match body {
            Ok(body) => match String::from_utf8(body.to_vec()) {
                Ok(body) => body,
                Err(_) if trust.is_none() => String::new(),
                Err(_) => return into_axum_response(invalid_request_response()),
            },
            Err(_) if trust.is_none() => String::new(),
            Err(_) => return into_axum_response(invalid_request_response()),
        }
    };
    let request = HttpRequest {
        method,
        path: uri.path().to_owned(),
        content_type: header(&headers, "content-type").map(str::to_owned),
        body,
        trust,
    };
    if request.path == "/api/v1/ai/chat/stream" {
        return handle_stream_request((*service).clone(), request).await;
    }
    match tokio::task::spawn_blocking(move || HttpAdapter::new((*service).clone()).handle(request))
        .await
    {
        Ok(response) => into_axum_response(response),
        Err(_) => internal_failure(),
    }
}

async fn handle_approval_request<A>(
    State(approvals): State<Arc<A>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response
where
    A: ApprovalDecisionTransport + Clone + Send + Sync + 'static,
{
    let trust = trust_context(&headers);
    let thread = trust_thread(&headers);
    let method = if method == Method::POST {
        HttpMethod::Post
    } else {
        HttpMethod::Other
    };
    let body = if method == HttpMethod::Other {
        String::new()
    } else {
        match body {
            Ok(body) => match String::from_utf8(body.to_vec()) {
                Ok(body) => body,
                Err(_) if trust.is_none() => String::new(),
                Err(_) => return into_axum_response(invalid_request_response()),
            },
            Err(_) if trust.is_none() => String::new(),
            Err(_) => return into_axum_response(invalid_request_response()),
        }
    };
    let request = HttpRequest {
        method,
        path: uri.path().to_owned(),
        content_type: header(&headers, "content-type").map(str::to_owned),
        body,
        trust,
    };
    // The canonical store blocks on its owning runtime, so the synchronous
    // adapter runs off the async workers like the turn adapter.
    match tokio::task::spawn_blocking(move || {
        let mut adapter = ApprovalDecisionAdapter::new((*approvals).clone(), unix_time_ms);
        adapter.handle(request, thread)
    })
    .await
    {
        Ok(response) => into_axum_response(response),
        Err(_) => internal_failure(),
    }
}

async fn handle_stream_request<S>(service: S, request: HttpRequest) -> Response
where
    S: TurnService + Clone + Send + Sync + 'static,
{
    let (decision_sender, decision_receiver) = oneshot::channel();
    let (body_sender, body_receiver) =
        mpsc::channel::<Result<Bytes, Infallible>>(STREAM_BUFFER_CAPACITY);
    tokio::task::spawn_blocking(move || {
        let mut decision_sender = Some(decision_sender);
        let mut body_sender = Some(body_sender);
        let delivery_aborted = Arc::new(AtomicBool::new(false));
        let cancellation_sender = body_sender
            .as_ref()
            .expect("stream body sender is initialized")
            .clone();
        let cancellation_state = Arc::clone(&delivery_aborted);
        let mut adapter = HttpAdapter::new(service);
        let response = adapter.handle_stream_controlled(
            request,
            &mut |chunk| {
                if let Some(sender) = decision_sender.take() {
                    let _ = sender.send(Ok(()));
                }
                if !chunk.is_empty() {
                    let delivery = body_sender
                        .as_ref()
                        .map(|sender| sender.try_send(Ok(Bytes::from(chunk))));
                    if delivery.is_some_and(|result| result.is_err()) {
                        delivery_aborted.store(true, Ordering::Release);
                        body_sender = None;
                    }
                }
            },
            &|| cancellation_state.load(Ordering::Acquire) || cancellation_sender.is_closed(),
        );
        if let Some(sender) = decision_sender.take() {
            let _ = sender.send(Err(response));
        }
    });
    match decision_receiver.await {
        Ok(Ok(())) => streaming_response(body_receiver),
        Ok(Err(response)) => into_axum_response(response),
        Err(_) => internal_failure(),
    }
}

fn streaming_response(receiver: mpsc::Receiver<Result<Bytes, Infallible>>) -> Response {
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(receiver)));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
}

// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

/// Production runtime state assembled once at startup.
///
/// The state holds the process's sole C-5 Turn authority root and only
/// distributes shared `ToolExecutionRuntimeRoot` handles (ADR-0003
/// TC-09/TC-12): every handle returned by one state shares one authority
/// catalog, so one Turn keeps exactly one 16-slot attempt budget and one
/// running D-7 in this process. T-2 wires the transport consumers; until then
/// the handles are exercised by the crate-internal boundary harness.
pub(crate) struct RuntimeState {
    tool_execution_root: crate::application::ToolExecutionRuntimeRoot,
}

impl RuntimeState {
    /// Assembles the runtime state at startup, issuing the process's sole C-5
    /// Turn authority root.
    pub(crate) fn assemble() -> Self {
        Self {
            tool_execution_root: crate::application::ToolExecutionRuntimeRoot::issue(),
        }
    }

    /// Distributes one shared handle to the held C-5 Turn authority root.
    #[cfg(test)]
    pub(crate) fn tool_execution_root(&self) -> crate::application::ToolExecutionRuntimeRoot {
        self.tool_execution_root.clone()
    }

    /// Returns the production tool-call executor over this state's sole C-5
    /// authority root, the empty production descriptor snapshot, the injected
    /// conditional terminal committer, the injected durable C-6 lease
    /// validator, the injected audit trail, and the injected canonical
    /// Turn-terminal probe: every model Tool call resolves against the empty
    /// inventory and is recorded as a typed denial with zero D-6/D-7 and zero
    /// dispatch, a configured capability commits its terminals through the
    /// durable store, both dispatch and authenticated interruption validate
    /// the bound generation against the durable lease before any D-7
    /// mutation, every terminal emits one correlated, bounded audit record
    /// through the trail, and the runner's Turn-terminal notification
    /// reclaims process-local authority only after the probe proves the
    /// canonical terminal (ADR-0003 TC-02/TC-07/TC-12/TC-13/TC-14, T-3).
    pub(crate) fn tool_call_executor<C, L, A, P>(
        &self,
        committer: C,
        lease: L,
        audits: A,
        terminals: P,
    ) -> tool_executor::BoundaryToolCallExecutor<C, L, A, P>
    where
        C: crate::application::AttemptCommitter
            + crate::application::DurableAttemptTransitions
            + crate::application::ExecutionAttemptInterruptionGuard
            + crate::application::ExecutionAttemptLiveness
            + Clone,
        L: crate::application::LeaseValidator + Clone + 'static,
        A: crate::application::ToolAuditTrail + Clone + 'static,
        P: crate::application::CanonicalTurnTerminal + Clone + 'static,
    {
        tool_executor::BoundaryToolCallExecutor::new(
            &self.tool_execution_root,
            crate::application::ToolConfigurationSnapshot::empty(),
            committer,
            lease,
            audits,
            terminals,
        )
    }

    /// Returns the background-terminal observer bound to this process's sole
    /// C-5 authority root and canonical terminal probe.
    pub(crate) fn terminal_observer<P>(&self, terminals: P) -> Arc<dyn TurnTerminalObserver>
    where
        P: crate::application::CanonicalTurnTerminal + Clone + Send + Sync + 'static,
    {
        Arc::new(tool_executor::AuthorityTerminalObserver::new(
            &self.tool_execution_root,
            terminals,
        ))
    }
}

fn trust_context(headers: &HeaderMap) -> Option<TrustContext> {
    let tenant_id = TenantId::new(header(headers, "x-koduck-tenant-id")?).ok()?;
    let trust = TrustContext::new(tenant_id, header(headers, "x-koduck-subject-id")?).ok()?;
    match headers.get(APPROVAL_SCOPES_HEADER) {
        None => Some(trust),
        // The configured gateway/Auth boundary validates signed claims and
        // injects the scope header as part of the validated context channel
        // (ADR-0003 TC-05, per repository-owner direction on 2026-08-14);
        // koduck-ai only seals what that boundary already validated. A
        // present-but-unreadable or malformed value invalidates the whole
        // identity rather than being silently downgraded to no scopes,
        // because the gateway never emits malformed context.
        Some(value) => {
            Some(trust.with_approval_scopes(gateway_validated_scopes(value.to_str().ok()?)?))
        }
    }
}

/// Seals the gateway-validated approval scopes carried by the trusted context
/// channel.
///
/// Returns `None` for any malformed value — empty tokens, whitespace or other
/// forbidden characters, oversized tokens, or more than
/// [`MAX_APPROVAL_SCOPES`] entries — so a malformed scope header yields an
/// invalid identity instead of a partially trusted one. Tokens are validated
/// exactly as delivered: surrounding whitespace is not normalized away,
/// because the gateway issues canonical comma-separated values only. The
/// count bound is enforced before a token is copied, so an over-count header
/// allocates at most [`MAX_APPROVAL_SCOPES`] tokens before rejection.
fn gateway_validated_scopes(raw: &str) -> Option<crate::domain::ApprovalScopes> {
    let mut scopes = Vec::new();
    for token in raw.split(',') {
        if scopes.len() == MAX_APPROVAL_SCOPES
            || token.is_empty()
            || token.len() > MAX_APPROVAL_SCOPE_BYTES
            || !token.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
        {
            return None;
        }
        scopes.push(token.to_owned());
    }
    Some(crate::domain::ApprovalScopes::from_validated(scopes))
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Extracts the well-formed Thread routing context for the approval-decision
/// route (ADR-0003 TC-05).
///
/// The header is client-supplied routing context, not authority: the canonical
/// lookup additionally requires the gateway-validated tenant, the requester
/// subject, and the approval identity, so an absent, malformed, or wrong
/// Thread value only fails closed as an indistinguishable `404` and can never
/// widen what a principal may resolve. The adapter receives only a validated
/// well-formed Thread identity or none.
fn trust_thread(headers: &HeaderMap) -> Option<ThreadId> {
    uuid::Uuid::parse_str(header(headers, THREAD_ROUTING_HEADER)?)
        .ok()
        .map(ThreadId::from_uuid)
}

fn into_axum_response(response: crate::adapters::http::HttpResponse) -> Response {
    let mut output = Response::new(Body::from(response.body));
    *output.status_mut() =
        StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for (name, value) in response.headers {
        let Ok(name) = HeaderName::try_from(name) else {
            return internal_failure();
        };
        let Ok(value) = HeaderValue::try_from(value) else {
            return internal_failure();
        };
        output.headers_mut().insert(name, value);
    }
    output
}

fn internal_failure() -> Response {
    let body = serde_json::json!({
        "type": "about:blank",
        "title": "Runtime unavailable",
        "status": 503,
        "code": "runtime-unavailable",
        "correlation_id": uuid::Uuid::new_v4().to_string(),
    })
    .to_string();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    response
}

/// A production runtime assembly or serving failure.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Process configuration was absent or invalid.
    #[error(transparent)]
    Config(#[from] RuntimeConfigError),
    /// `PostgreSQL` connection, migration, or query setup failed.
    #[error("PostgreSQL runtime setup failed")]
    Database(#[source] sqlx::Error),
    /// `PostgreSQL` connection or migration exceeded the approved attempt deadline.
    #[error("PostgreSQL runtime setup timed out")]
    DatabaseTimeout,
    /// The configured provider client could not be constructed.
    #[error("provider client setup failed")]
    ProviderClient(#[source] reqwest::Error),
    /// The global orphan-reconciliation worker could not be started.
    #[error("reconciliation worker startup failed")]
    ReconciliationWorker(#[source] std::io::Error),
    /// The configured listener address could not be bound.
    #[error("AI listener bind failed")]
    Bind(#[source] std::io::Error),
    /// The HTTP server stopped with an I/O failure.
    #[error("AI HTTP server failed")]
    Serve(#[source] std::io::Error),
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("bind_addr", &self.bind_addr)
            .field("database_url", &"[REDACTED]")
            .field("provider_base_url", &self.provider_base_url)
            .field("provider_model", &self.provider_model)
            .field("provider_api_key", &"[REDACTED]")
            .finish()
    }
}

/// A rejected runtime environment value.
#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    /// A required environment variable was absent or blank.
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    /// The configured bind address could not be parsed.
    #[error("invalid KODUCK_AI_BIND_ADDR")]
    InvalidBindAddress(#[source] AddrParseError),
    /// The provider base URL was invalid or did not use HTTPS.
    #[error("invalid KODUCK_AI_OPENAI_BASE_URL")]
    InvalidProviderBaseUrl,
}

fn required<'a>(
    environment: &'a BTreeMap<String, String>,
    name: &'static str,
) -> Result<&'a str, RuntimeConfigError> {
    environment
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(RuntimeConfigError::Missing(name))
}

#[cfg(test)]
#[path = "../../tests/internal/runtime_mod.rs"]
mod tests;
