// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Framework-neutral contract harness for the approval-decision v1 route.

use std::collections::HashMap;

use koduck_ai::adapters::http::HttpMethod;
use koduck_ai::adapters::http::approvals::ApprovalDecisionAdapter;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ApprovalDecisionResolution, ApprovalInsertResolution, ApprovalRecordStore, ApprovalStoreError,
    ToolAuthorizationService, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus, AttemptId, ExactActionBinding,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{
    ApprovalScopes, LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId,
};

/// In-memory canonical D-6 double with the same conditional semantics.
struct MemoryApprovals {
    rows: HashMap<(String, ApprovalId), MemoryRow>,
    mutations: usize,
}

struct MemoryRow {
    thread_id: koduck_ai::domain::ThreadId,
    requester_subject: String,
    expires_at_millis: u64,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
}

impl ApprovalRecordStore for MemoryApprovals {
    fn insert_requested(
        &mut self,
        request: &ApprovalRequest,
        requester_subject: &str,
    ) -> Result<ApprovalInsertResolution, ApprovalStoreError> {
        let key = (
            request.tenant_id().as_str().to_owned(),
            request.approval_id(),
        );
        if let Some(row) = self.rows.get(&key) {
            return Ok(ApprovalInsertResolution::Existing {
                status: row.status,
                decision: row.decision,
                version: row.version,
            });
        }
        self.rows.insert(
            key,
            MemoryRow {
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
        thread_id: koduck_ai::domain::ThreadId,
        requester_subject: &str,
        decision: ApprovalDecision,
        _approver: &koduck_ai::domain::execution::ApproverId,
        decided_at_millis: u64,
    ) -> Result<ApprovalDecisionResolution, ApprovalStoreError> {
        let key = (tenant_id.as_str().to_owned(), approval_id);
        let Some(row) = self.rows.get_mut(&key) else {
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
        self.mutations += 1;
        if decided_at_millis >= row.expires_at_millis {
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

fn seeded_store() -> (MemoryApprovals, ApprovalRequest) {
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
            parse_action_parameters(r#"{"value":1}"#).expect("valid parameters"),
        )
        .expect("valid action"),
    )
    .expect("valid binding");
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{"value":{"type":"number"}},"required":["value"],"additionalProperties":false}"#,
        )
        .expect("valid schema"),
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    let sealed = ToolAuthorizationService::new(FixtureConfiguration {
        descriptor,
        profile,
    })
    .authorize_binding(binding)
    .expect("fixture binding is policy-authorized");
    let request = ApprovalRequest::new(sealed, 1_000, 60_000).expect("valid approval");
    let mut store = MemoryApprovals {
        rows: HashMap::new(),
        mutations: 0,
    };
    assert_eq!(
        store.insert_requested(&request, "requester"),
        Ok(ApprovalInsertResolution::Inserted)
    );
    (store, request)
}

struct FixtureConfiguration {
    descriptor: CapabilityDescriptor,
    profile: PermissionProfile,
}

impl ToolPolicyConfiguration for FixtureConfiguration {
    fn descriptor_for(&self, _action: &Action) -> Option<&CapabilityDescriptor> {
        Some(&self.descriptor)
    }

    fn profile_for(&self, profile_id: &str, profile_version: &str) -> Option<&PermissionProfile> {
        (self.profile.id() == profile_id && self.profile.version() == profile_version)
            .then_some(&self.profile)
    }
}

fn scoped_trust(tenant: &str, subject: &str) -> TrustContext {
    TrustContext::new(TenantId::new(tenant).expect("valid tenant"), subject)
        .expect("valid principal")
        .with_approval_scopes(ApprovalScopes::from_validated([
            koduck_ai::application::TOOL_APPROVAL_SCOPE,
        ]))
}

fn request(
    trust: Option<TrustContext>,
    approval_id: ApprovalId,
    body: &str,
) -> koduck_ai::adapters::http::HttpRequest {
    koduck_ai::adapters::http::HttpRequest {
        method: HttpMethod::Post,
        path: format!("/api/v1/ai/approvals/{}/decisions", approval_id.as_uuid()),
        content_type: Some("application/json".to_owned()),
        body: body.to_owned(),
        trust,
    }
}

#[test]
fn approval_decision_v1_contract() {
    let (store, approval) = seeded_store();
    let route = koduck_ai::application::ApprovalDecisionRoute::new(store);
    let mut adapter = ApprovalDecisionAdapter::new(route, || 2_000);
    let approval_id = approval.approval_id();
    let thread = approval.binding().thread_id();
    let body = r#"{"decision":"accepted"}"#;

    let decide = |adapter: &mut ApprovalDecisionAdapter<_>, trust, thread| {
        adapter.handle(request(trust, approval_id, body), thread)
    };

    // Missing identity is 401 with the owned problem contract.
    let missing = decide(&mut adapter, None, thread);
    assert_eq!(missing.status, 401);
    assert_eq!(missing.header("WWW-Authenticate"), Some("Bearer"));

    // Unscoped principals and other tenants learn nothing, with zero mutation.
    let unscoped = TrustContext::new(TenantId::new("tenant-a").expect("valid tenant"), "subject")
        .expect("valid principal");
    assert_eq!(decide(&mut adapter, Some(unscoped), thread).status, 404);
    assert_eq!(
        decide(
            &mut adapter,
            Some(scoped_trust("tenant-b", "subject")),
            thread
        )
        .status,
        404
    );
    assert_eq!(
        adapter.service().store().mutations,
        0,
        "404 cases mutate nothing"
    );

    // Malformed bodies are rejected before any decision.
    for body in [
        "{}",
        r#"{"decision":"accepted","extra":1}"#,
        r#"{"decision":"maybe"}"#,
        "not json",
        r#"{"decision":"declined","decision":"accepted"}"#,
    ] {
        let invalid = adapter.handle(
            request(
                Some(scoped_trust("tenant-a", "requester")),
                approval_id,
                body,
            ),
            thread,
        );
        assert_eq!(invalid.status, 400, "body {body} must be invalid");
    }
    assert_eq!(adapter.service().store().mutations, 0);

    // A scoped owning subject reached through a different trusted Thread
    // learns nothing: indistinguishable 404 with zero mutation.
    let wrong_thread = decide(
        &mut adapter,
        Some(scoped_trust("tenant-a", "requester")),
        koduck_ai::domain::ThreadId::new(),
    );
    assert_eq!(wrong_thread.status, 404);

    // A scoped same-tenant principal that does not own the approval learns
    // nothing: indistinguishable 404 with zero mutation.
    let wrong_owner = decide(
        &mut adapter,
        Some(scoped_trust("tenant-a", "intruder")),
        thread,
    );
    assert_eq!(wrong_owner.status, 404);
    assert_eq!(adapter.service().store().mutations, 0);

    // A valid decision commits and returns the exact terminal projection.
    let valid = decide(
        &mut adapter,
        Some(scoped_trust("tenant-a", "requester")),
        thread,
    );
    assert_eq!(valid.status, 200);
    assert_eq!(
        valid.body,
        format!(
            "{{\"approval_id\":\"{id}\",\"status\":\"accepted\",\"decision\":\"accepted\",\"version\":2}}",
            id = approval_id.as_uuid()
        )
    );

    // An identical replay from the owning subject returns the same terminal
    // version; a conflicting decision is 409.
    let duplicate = decide(
        &mut adapter,
        Some(scoped_trust("tenant-a", "requester")),
        thread,
    );
    assert_eq!(duplicate.status, 200);
    assert_eq!(duplicate.body, valid.body);
    let conflict = adapter.handle(
        request(
            Some(scoped_trust("tenant-a", "requester")),
            approval_id,
            r#"{"decision":"declined"}"#,
        ),
        thread,
    );
    assert_eq!(conflict.status, 409);

    // Unknown approval identities are indistinguishable 404.
    let unknown_id = ApprovalId::new();
    let unknown = adapter.handle(
        request(
            Some(scoped_trust("tenant-a", "requester")),
            unknown_id,
            body,
        ),
        thread,
    );
    assert_eq!(unknown.status, 404);

    // Exactly one mutation happened across the whole contract run.
    assert_eq!(adapter.service().store().mutations, 1);
}
