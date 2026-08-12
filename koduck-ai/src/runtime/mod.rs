// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Production runtime configuration and executable assembly.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;
use std::net::{AddrParseError, SocketAddr};
use std::sync::Arc;

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

use crate::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use crate::adapters::http::{
    HttpAdapter, HttpMethod, HttpRequest, TurnService, invalid_request_response,
};
use crate::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use crate::application::TurnRunner;
use crate::domain::{TenantId, TrustContext};

const BIND_ADDR: &str = "KODUCK_AI_BIND_ADDR";
const DATABASE_URL: &str = "KODUCK_AI_DATABASE_URL";
const PROVIDER_BASE_URL: &str = "KODUCK_AI_OPENAI_BASE_URL";
const PROVIDER_MODEL: &str = "KODUCK_AI_OPENAI_MODEL";
const PROVIDER_API_KEY: &str = "KODUCK_AI_OPENAI_API_KEY";
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
        if provider_url.scheme() != "https" {
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

/// Builds the three owned v1 Axum routes around the framework-neutral adapter.
pub fn build_router<S>(service: S) -> Router
where
    S: TurnService + Clone + Send + Sync + 'static,
{
    let state = Arc::new(service);
    Router::new()
        .route("/api/v1/ai/chat", any(handle_request::<S>))
        .route("/api/v1/ai/chat/stream", any(handle_request::<S>))
        .route(
            "/api/v1/ai/turns/{turn_id}/interrupt",
            any(handle_request::<S>),
        )
        .with_state(state)
}

/// Connects production adapters, applies the owned migration, and serves HTTP.
///
/// # Errors
///
/// Returns [`RuntimeError`] when `PostgreSQL`, provider-client construction,
/// listener binding, or HTTP serving fails.
pub async fn run(config: RuntimeConfig) -> Result<(), RuntimeError> {
    let pool = PgPoolOptions::new()
        .connect(config.database_url())
        .await
        .map_err(RuntimeError::Database)?;
    sqlx::raw_sql(include_str!("../../migrations/0001_cand_1_history.sql"))
        .execute(&pool)
        .await
        .map_err(RuntimeError::Database)?;
    let runtime = tokio::runtime::Handle::current();
    let history = PostgresTurnHistory::new(SqlxPostgresExecutor::new(pool, runtime.clone()));
    let _reconciliation_worker = history
        .start_reconciliation_worker()
        .map_err(RuntimeError::ReconciliationWorker)?;
    let client = reqwest::Client::builder()
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
    let runner = TurnRunner::new(provider, history);
    let listener = tokio::net::TcpListener::bind(config.bind_addr())
        .await
        .map_err(RuntimeError::Bind)?;
    axum::serve(listener, build_router(runner))
        .await
        .map_err(RuntimeError::Serve)
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
        let mut adapter = HttpAdapter::new(service);
        let response = adapter.handle_stream(request, &mut |chunk| {
            if let Some(sender) = decision_sender.take() {
                let _ = sender.send(Ok(()));
            }
            if !chunk.is_empty() {
                let delivery = body_sender
                    .as_ref()
                    .map(|sender| sender.try_send(Ok(Bytes::from(chunk))));
                if delivery.is_some_and(|result| result.is_err()) {
                    body_sender = None;
                }
            }
        });
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

fn trust_context(headers: &HeaderMap) -> Option<TrustContext> {
    let tenant_id = TenantId::new(header(headers, "x-koduck-tenant-id")?).ok()?;
    TrustContext::new(tenant_id, header(headers, "x-koduck-subject-id")?).ok()
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
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
