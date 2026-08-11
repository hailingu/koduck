// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Production runtime configuration and executable assembly.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{AddrParseError, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::Response;
use axum::routing::post;
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

use crate::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use crate::adapters::http::{HttpAdapter, HttpMethod, HttpRequest, TurnService};
use crate::adapters::provider::{OpenAiCompatibleProvider, ReqwestOpenAiTransport};
use crate::application::TurnRunner;
use crate::domain::{TenantId, TrustContext};

const BIND_ADDR: &str = "KODUCK_AI_BIND_ADDR";
const DATABASE_URL: &str = "KODUCK_AI_DATABASE_URL";
const PROVIDER_BASE_URL: &str = "KODUCK_AI_OPENAI_BASE_URL";
const PROVIDER_MODEL: &str = "KODUCK_AI_OPENAI_MODEL";
const PROVIDER_API_KEY: &str = "KODUCK_AI_OPENAI_API_KEY";

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
        Ok(Self {
            bind_addr,
            database_url: required(environment, DATABASE_URL)?.to_owned(),
            provider_base_url: required(environment, PROVIDER_BASE_URL)?.to_owned(),
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
    S: TurnService + Send + 'static,
{
    let state = Arc::new(Mutex::new(HttpAdapter::new(service)));
    Router::new()
        .route("/api/v1/ai/chat", post(handle_request::<S>))
        .route("/api/v1/ai/chat/stream", post(handle_request::<S>))
        .route(
            "/api/v1/ai/turns/{turn_id}/interrupt",
            post(handle_request::<S>),
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
    State(adapter): State<Arc<Mutex<HttpAdapter<S>>>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response
where
    S: TurnService + Send + 'static,
{
    let request = HttpRequest {
        method: HttpMethod::Post,
        path: uri.path().to_owned(),
        content_type: header(&headers, "content-type").map(str::to_owned),
        body: String::from_utf8_lossy(&body).into_owned(),
        trust: trust_context(&headers),
    };
    match tokio::task::spawn_blocking(move || {
        adapter
            .lock()
            .map_err(|_| ())
            .map(|mut adapter| adapter.handle(request))
    })
    .await
    {
        Ok(Ok(response)) => into_axum_response(response),
        Ok(Err(())) | Err(_) => internal_failure(),
    }
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
    let mut response = Response::new(Body::from(
        "{\"type\":\"about:blank\",\"title\":\"Runtime unavailable\",\"status\":503,\"code\":\"runtime-unavailable\"}",
    ));
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
