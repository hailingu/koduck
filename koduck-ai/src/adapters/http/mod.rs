// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Owned HTTP/SSE v1 presentation contract around the application turn kernel.

mod wire;

use std::collections::BTreeMap;

use thiserror::Error;
use uuid::Uuid;

use crate::application::{
    HistoryError, ModelProvider, TurnCommand, TurnHistory, TurnResult, TurnRunError, TurnRunner,
    TurnStreamEvent,
};
use crate::domain::{TrustContext, TurnId};

use self::wire::{
    interrupt_body, parse_turn_request, problem_body, sse_body, stream_error_body,
    stream_event_body, sync_body,
};

/// Supported HTTP methods for the owned v1 routes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    /// HTTP POST.
    Post,
    /// Any method not supported by the owned v1 routes.
    Other,
}

/// Framework-neutral request data supplied by the configured presentation server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    /// Request method.
    pub method: HttpMethod,
    /// Absolute request path without query parameters.
    pub path: String,
    /// Parsed request content type, when present.
    pub content_type: Option<String>,
    /// Request bytes decoded as UTF-8 by the presentation server.
    pub body: String,
    /// Validated identity supplied by the gateway/Auth boundary.
    pub trust: Option<TrustContext>,
}

/// Framework-neutral response emitted by the owned v1 adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    /// Numeric HTTP status.
    pub status: u16,
    /// Exact response headers required by the contract.
    pub headers: BTreeMap<String, String>,
    /// JSON or SSE response body.
    pub body: String,
}

impl HttpResponse {
    /// Returns a response header by exact contract name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// A presentation-facing service failure with stable HTTP mapping.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ServiceError {
    /// The tenant-scoped turn is unknown or not owned by the caller.
    #[error("turn not found")]
    NotFound,
    /// The requested turn already has a terminal outcome.
    #[error("turn already terminal")]
    AlreadyTerminal,
    /// Canonical history is unavailable.
    #[error("durability unavailable")]
    DurabilityUnavailable,
    /// The provider failed before a normal owned result was available.
    #[error("provider unavailable")]
    ProviderUnavailable,
}

/// Presentation-owned service boundary used by the REST/SSE adapter.
pub trait TurnService {
    /// Executes one validated command through the provider-neutral kernel.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] when the kernel cannot expose a normal result.
    fn execute(&mut self, command: TurnCommand) -> Result<TurnResult, ServiceError>;

    /// Executes one command while reporting durable stream events incrementally.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError`] when no normal owned result can be exposed.
    fn execute_stream(
        &mut self,
        command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, ServiceError> {
        let result = self.execute(command)?;
        observer(TurnStreamEvent::Started {
            thread_id: result.thread_id,
            turn_id: result.turn_id,
        });
        for item in &result.published {
            observer(TurnStreamEvent::Item {
                thread_id: result.thread_id,
                turn_id: result.turn_id,
                item: item.clone(),
            });
        }
        Ok(result)
    }

    /// Requests interruption of one tenant-owned active turn.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::NotFound`] for both unknown and non-owned turns,
    /// or [`ServiceError::AlreadyTerminal`] for a known terminal turn.
    fn interrupt(&mut self, trust: &TrustContext, turn_id: TurnId) -> Result<(), ServiceError>;
}

impl<P, H> TurnService for TurnRunner<P, H>
where
    P: ModelProvider,
    H: TurnHistory,
{
    fn execute(&mut self, command: TurnCommand) -> Result<TurnResult, ServiceError> {
        TurnRunner::execute(self, command).map_err(|error| map_turn_run_error(&error))
    }

    fn execute_stream(
        &mut self,
        command: TurnCommand,
        observer: &mut dyn FnMut(TurnStreamEvent),
    ) -> Result<TurnResult, ServiceError> {
        self.execute_with_observer(command, observer)
            .map_err(|error| map_turn_run_error(&error))
    }

    fn interrupt(&mut self, trust: &TrustContext, turn_id: TurnId) -> Result<(), ServiceError> {
        self.request_interrupt(trust, turn_id)
            .map_err(|error| map_turn_run_error(&error))
    }
}

/// Dispatches the three owned v1 routes without leaking transport types inward.
pub struct HttpAdapter<S> {
    service: S,
}

impl<S: TurnService> HttpAdapter<S> {
    /// Creates an adapter around the application-facing service boundary.
    #[must_use]
    pub const fn new(service: S) -> Self {
        Self { service }
    }

    /// Handles one owned v1 request.
    #[must_use]
    pub fn handle(&mut self, request: HttpRequest) -> HttpResponse {
        let Some(trust) = request.trust else {
            return problem(401, "invalid-identity", true);
        };
        if request.method != HttpMethod::Post {
            return problem(405, "method-not-allowed", false);
        }
        if let Some(turn_id) = interrupt_turn_id(&request.path) {
            return self.handle_interrupt(&trust, turn_id, &request.body);
        }
        if request.content_type.as_deref() != Some("application/json") {
            return problem(400, "invalid-request", false);
        }

        let Ok(command) = parse_turn_request(&request.body, trust) else {
            return problem(400, "invalid-request", false);
        };
        match request.path.as_str() {
            "/api/v1/ai/chat" => self.execute(command, false),
            "/api/v1/ai/chat/stream" => self.execute(command, true),
            _ => problem(404, "not-found", false),
        }
    }

    /// Handles the SSE route and emits each durable event as it becomes available.
    #[must_use]
    pub fn handle_stream(
        &mut self,
        request: HttpRequest,
        emit: &mut dyn FnMut(String),
    ) -> HttpResponse {
        let Some(trust) = request.trust else {
            return problem(401, "invalid-identity", true);
        };
        if request.method != HttpMethod::Post {
            return problem(405, "method-not-allowed", false);
        }
        if request.path != "/api/v1/ai/chat/stream"
            || request.content_type.as_deref() != Some("application/json")
        {
            return problem(400, "invalid-request", false);
        }
        let Ok(command) = parse_turn_request(&request.body, trust) else {
            return problem(400, "invalid-request", false);
        };
        let mut started = false;
        let mut terminal_emitted = false;
        let result = self.service.execute_stream(command, &mut |event| {
            started = true;
            terminal_emitted |= matches!(
                &event,
                TurnStreamEvent::Item { item, .. }
                    if matches!(
                        &item.payload,
                        crate::domain::ItemPayload::Terminal(_)
                    )
            );
            emit(stream_event_body(event));
        });
        match result {
            Ok(_) => response(200, "text/event-stream", String::new()),
            Err(_) if terminal_emitted => response(200, "text/event-stream", String::new()),
            Err(error) if started => {
                let problem = map_service_error(&error);
                emit(stream_error_body(&problem.body));
                response(200, "text/event-stream", String::new())
            }
            Err(error) => map_service_error(&error),
        }
    }

    fn execute(&mut self, command: TurnCommand, stream: bool) -> HttpResponse {
        match self.service.execute(command) {
            Ok(result) if stream => response(200, "text/event-stream", sse_body(&result)),
            Ok(result) if result.status != crate::domain::TurnStatus::Completed => {
                map_service_error(&ServiceError::ProviderUnavailable)
            }
            Ok(result) => response(200, "application/json", sync_body(&result)),
            Err(error) => map_service_error(&error),
        }
    }

    fn handle_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
        body: &str,
    ) -> HttpResponse {
        if !body.is_empty() {
            return problem(400, "invalid-request", false);
        }
        match self.service.interrupt(trust, turn_id) {
            Ok(()) => response(202, "application/json", interrupt_body(turn_id)),
            Err(error) => map_service_error(&error),
        }
    }
}

/// Returns the owned invalid-request problem response for transport validation.
pub(crate) fn invalid_request_response() -> HttpResponse {
    problem(400, "invalid-request", false)
}

fn interrupt_turn_id(path: &str) -> Option<TurnId> {
    let value = path
        .strip_prefix("/api/v1/ai/turns/")?
        .strip_suffix("/interrupt")?;
    Uuid::parse_str(value).ok().map(TurnId::from_uuid)
}

fn map_service_error(error: &ServiceError) -> HttpResponse {
    match error {
        ServiceError::NotFound => problem(404, "not-found", false),
        ServiceError::AlreadyTerminal => problem(409, "turn-already-terminal", false),
        ServiceError::DurabilityUnavailable => problem(503, "durability-unavailable", false),
        ServiceError::ProviderUnavailable => problem(503, "provider-unavailable", false),
    }
}

fn map_turn_run_error(error: &TurnRunError) -> ServiceError {
    match error {
        TurnRunError::Durability(_) | TurnRunError::History(HistoryError::Unavailable) => {
            ServiceError::DurabilityUnavailable
        }
        TurnRunError::History(HistoryError::NotFound | HistoryError::Fenced) => {
            ServiceError::NotFound
        }
        TurnRunError::History(HistoryError::AlreadyTerminal) => ServiceError::AlreadyTerminal,
        TurnRunError::Provider(_) | TurnRunError::Transition(_) => {
            ServiceError::ProviderUnavailable
        }
    }
}

fn response(status: u16, content_type: &str, body: String) -> HttpResponse {
    HttpResponse {
        status,
        headers: BTreeMap::from([("Content-Type".to_owned(), content_type.to_owned())]),
        body,
    }
}

fn problem(status: u16, code: &str, authenticate: bool) -> HttpResponse {
    let mut response = response(
        status,
        "application/problem+json",
        problem_body(status, code),
    );
    if authenticate {
        response
            .headers
            .insert("WWW-Authenticate".to_owned(), "Bearer".to_owned());
    }
    response
}
