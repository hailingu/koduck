// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Owned approval-decision v1 presentation route.

use uuid::Uuid;

use crate::application::{ApprovalDecisionOutcome, ApprovalDecisionRoute};
use crate::domain::TrustContext;
use crate::domain::execution::{ApprovalDecision, ApprovalId};

use super::{HttpMethod, HttpRequest, HttpResponse};

/// Presentation-owned decision transport consumed by the approval route.
pub trait ApprovalDecisionTransport {
    /// Applies one authenticated decision at the supplied decision time.
    fn decide(
        &mut self,
        trust: &TrustContext,
        thread_id: crate::domain::ThreadId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
    ) -> ApprovalDecisionOutcome;
}

impl<S> ApprovalDecisionTransport for ApprovalDecisionRoute<S>
where
    S: crate::application::ApprovalRecordStore,
{
    fn decide(
        &mut self,
        trust: &TrustContext,
        thread_id: crate::domain::ThreadId,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
    ) -> ApprovalDecisionOutcome {
        Self::decide(
            self,
            trust,
            thread_id,
            approval_id,
            decision,
            decided_at_millis,
        )
    }
}

/// Dispatches the owned approval-decision v1 route.
pub struct ApprovalDecisionAdapter<S> {
    service: S,
    now_millis: fn() -> u64,
}

impl<S: ApprovalDecisionTransport> ApprovalDecisionAdapter<S> {
    /// Creates the adapter around the decision transport and decision clock.
    #[must_use]
    pub const fn new(service: S, now_millis: fn() -> u64) -> Self {
        Self {
            service,
            now_millis,
        }
    }

    /// Returns the owned decision transport for crate-internal inspection.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "crate-internal test inspection only")
    )]
    pub(crate) fn service(&self) -> &S {
        &self.service
    }

    /// Handles one owned approval-decision request.
    ///
    /// `trusted_thread` is the Thread routing context the presentation server
    /// validated as a well-formed Thread identity. An absent context is
    /// indistinguishable from a mismatched one: the route resolves nothing,
    /// mutates no record, and exposes no approval existence (ADR-0003 TC-05).
    #[must_use]
    pub fn handle(
        &mut self,
        request: HttpRequest,
        trusted_thread: Option<crate::domain::ThreadId>,
    ) -> HttpResponse {
        let Some(trust) = request.trust else {
            return problem(401, "invalid-identity", true);
        };
        if request.method != HttpMethod::Post {
            return problem(405, "method-not-allowed", false);
        }
        let Some(approval_id) = approval_decision_id(&request.path) else {
            return problem(404, "not-found", false);
        };
        let Some(trusted_thread) = trusted_thread else {
            return problem(404, "not-found", false);
        };
        if !is_json_content_type(request.content_type.as_deref()) {
            return problem(400, "invalid-request", false);
        }
        let Some(decision) = parse_decision_body(&request.body) else {
            return problem(400, "invalid-request", false);
        };
        match self.service.decide(
            &trust,
            trusted_thread,
            approval_id,
            decision,
            (self.now_millis)(),
        ) {
            ApprovalDecisionOutcome::Resolved {
                status,
                decision,
                version,
            } => response(
                200,
                "application/json",
                format!(
                    "{{\"approval_id\":\"{id}\",\"status\":\"{status}\",\"decision\":\"{decision}\",\"version\":{version}}}",
                    id = approval_id.as_uuid(),
                    status = status_code(status),
                    decision = decision_code(decision),
                ),
            ),
            ApprovalDecisionOutcome::Conflict { .. } => {
                problem(409, "approval-already-resolved", false)
            }
            ApprovalDecisionOutcome::NotFound => problem(404, "not-found", false),
            ApprovalDecisionOutcome::Unavailable => problem(503, "durability-unavailable", false),
        }
    }
}

/// Returns the decision route's approval identity, when the path is the owned
/// v1 decision route.
pub fn approval_decision_id(path: &str) -> Option<ApprovalId> {
    let value = path
        .strip_prefix("/api/v1/ai/approvals/")?
        .strip_suffix("/decisions")?;
    Uuid::parse_str(value).ok().map(ApprovalId::from_uuid)
}

/// Parses the exact decision body: one JSON object with only `decision`.
///
/// The typed deserializer rejects duplicate members (`duplicate field`) and
/// unknown members (`deny_unknown_fields`) instead of letting a collapsed
/// last-value rewrite commit a decision the caller did not send.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionBody {
    decision: String,
}

fn parse_decision_body(body: &str) -> Option<ApprovalDecision> {
    let parsed: DecisionBody = serde_json::from_str(body).ok()?;
    match parsed.decision.as_str() {
        "accepted" => Some(ApprovalDecision::Accepted),
        "declined" => Some(ApprovalDecision::Declined),
        "cancelled" => Some(ApprovalDecision::Cancelled),
        _ => None,
    }
}

fn decision_code(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Accepted => "accepted",
        ApprovalDecision::Declined => "declined",
        ApprovalDecision::Cancelled => "cancelled",
    }
}

fn status_code(status: crate::domain::execution::ApprovalStatus) -> &'static str {
    match status {
        crate::domain::execution::ApprovalStatus::Requested => "requested",
        crate::domain::execution::ApprovalStatus::Accepted => "accepted",
        crate::domain::execution::ApprovalStatus::Declined => "declined",
        crate::domain::execution::ApprovalStatus::Cancelled => "cancelled",
        crate::domain::execution::ApprovalStatus::Expired => "expired",
    }
}

fn is_json_content_type(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

fn response(status: u16, content_type: &str, body: String) -> HttpResponse {
    HttpResponse {
        status,
        headers: [("Content-Type".to_owned(), content_type.to_owned())].into(),
        body,
    }
}

fn problem(status: u16, code: &str, authenticate: bool) -> HttpResponse {
    let correlation = Uuid::new_v4();
    let body = format!(
        "{{\"type\":\"about:blank\",\"title\":\"{title}\",\"status\":{status},\"code\":\"{code}\",\"correlation_id\":\"{correlation}\"}}",
        title = problem_title(status),
    );
    let mut response = response(status, "application/problem+json", body);
    if authenticate {
        response
            .headers
            .insert("WWW-Authenticate".to_owned(), "Bearer".to_owned());
    }
    response
}

fn problem_title(status: u16) -> &'static str {
    match status {
        400 => "Invalid request",
        401 => "Invalid identity",
        404 => "Not found",
        405 => "Method not allowed",
        409 => "Approval already resolved",
        _ => "Approval store unavailable",
    }
}
