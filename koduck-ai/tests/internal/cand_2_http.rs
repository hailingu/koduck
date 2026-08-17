// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Framework-neutral contract harness for the approval-decision v1 route.

use std::collections::HashMap;

use koduck_ai::adapters::http::HttpMethod;
use koduck_ai::adapters::http::approvals::ApprovalDecisionAdapter;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, ApprovalAuthorizer, ApprovalDecisionResolution, ApprovalInsertResolution,
    ApprovalRecordStore, ApprovalStoreError, AttemptCommitError, AttemptCommitResult,
    AttemptCommitter, CancelAcknowledgement, CancelPermit, CanonicalAttemptTerminal,
    DispatchPermit, EffectState, ExecutionCoordinator, ExecutionFailure, ExecutionPending,
    ExecutionPreparationError, ExecutionResponse, ExecutionResponseBuilder, ExecutorError,
    IsolatedExecutor, LeaseCheck, LeaseValidator, ToolAuthorizationService, ToolCallError,
    ToolCallInputs, ToolExecutionDriver, ToolExecutionOutcome, ToolExecutionRuntime,
    ToolPolicyConfiguration, ToolProjection, ToolProjectionError, ToolProjectionSink,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus, AttemptId, ExactActionBinding,
    ExecutionStatus,
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

// One cohesive AC-6 contract harness: every identity, body, ownership, and
// terminal case shares the seeded canonical double, and the ADR acceptance
// command pins this exact single test name. Splitting it would duplicate the
// security-sensitive fixture without creating an independent test boundary.
#[allow(clippy::too_many_lines)]
#[test]
fn approval_decision_v1_contract() {
    let (store, approval) = seeded_store();
    let route = koduck_ai::application::ApprovalDecisionRoute::new(store);
    let mut adapter = ApprovalDecisionAdapter::new(route, || 2_000);
    let approval_id = approval.approval_id();
    let thread = approval.binding().thread_id();
    let body = r#"{"decision":"accepted"}"#;

    let decide = |adapter: &mut ApprovalDecisionAdapter<_>, trust, thread| {
        adapter.handle(request(trust, approval_id, body), Some(thread))
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

    // An absent Thread routing context is indistinguishable from a mismatched
    // one: the route resolves nothing and mutates no record (TC-05).
    assert_eq!(
        adapter
            .handle(
                request(
                    Some(scoped_trust("tenant-a", "requester")),
                    approval_id,
                    body
                ),
                None
            )
            .status,
        404
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
            Some(thread),
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
        Some(thread),
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
        Some(thread),
    );
    assert_eq!(unknown.status, 404);

    // Exactly one mutation happened across the whole contract run.
    assert_eq!(adapter.service().store().mutations, 1);
}

struct CurrentLease;
impl LeaseValidator for CurrentLease {
    fn check_current(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
    ) -> LeaseCheck {
        LeaseCheck::Current
    }
}
struct OneShotExecutor {
    calls: usize,
}
impl IsolatedExecutor for OneShotExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.calls += 1;
        let mut response = ExecutionResponseBuilder::new(EffectState::Started);
        response
            .push_chunk(b"ok")
            .expect("fixture response is bounded");
        response.finish()
    }
    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}
struct WinningCommitter {
    calls: usize,
}
impl AttemptCommitter for WinningCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.calls += 1;
        Ok(AttemptCommitResult::Won)
    }
}
struct AllowApprovals;
impl ApprovalAuthorizer for AllowApprovals {
    fn can_resolve_tool_approval(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> bool {
        true
    }
}

/// One phase of an observed projection event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionPhase {
    Append,
    Publish,
}

/// Recording D-3 sink that observes the append-before-publish ordering.
#[derive(Default)]
struct RecordingProjections {
    events: Vec<(ProjectionPhase, ToolProjection)>,
}

impl ToolProjectionSink for RecordingProjections {
    fn append(&mut self, projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        self.events
            .push((ProjectionPhase::Append, projection.clone()));
        Ok(())
    }

    fn publish(&mut self, projection: &ToolProjection) {
        self.events
            .push((ProjectionPhase::Publish, projection.clone()));
    }
}

/// AC-7: approval and execution projections are ordered durable views whose
/// publication follows their append, and they are never authority.
// One cohesive AC-7 harness: the ordered-sequence, append-before-publish,
// deleted-projection, and replay/forgery legs share one fixture; the ADR
// acceptance command pins this exact test name.
#[allow(clippy::too_many_lines)]
#[test]
fn projections_append_before_publish() {
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
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
    let configuration = FixtureConfiguration {
        descriptor: descriptor.clone(),
        profile: profile.clone(),
    };
    let inputs = ToolCallInputs {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
            parse_action_parameters("{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: 600_000,
    };
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "requester",
    )
    .expect("valid principal")
    .with_approval_scopes(ApprovalScopes::from_validated([
        koduck_ai::application::TOOL_APPROVAL_SCOPE,
    ]));

    let mut decisions = 0;
    let mut decision = |_request: &ApprovalRequest| {
        decisions += 1;
        (ApprovalDecision::Accepted, 2_000)
    };
    let mut projections = RecordingProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &inputs,
        &trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    )
    .expect("the approval-required call completes");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));

    // The ordered projection sequence covers requested, accepted, running,
    // and succeeded views with canonical identities and versions.
    let appended: Vec<ToolProjection> = projections
        .events
        .iter()
        .filter(|(phase, _)| *phase == ProjectionPhase::Append)
        .map(|(_, projection)| projection.clone())
        .collect();
    let approval_id = match appended.first() {
        Some(ToolProjection::ApprovalStatus { approval_id, .. }) => *approval_id,
        other => panic!("the first projection is the requested D-6 view: {other:?}"),
    };
    let attempt_id = match appended.get(2) {
        Some(ToolProjection::ToolCall { attempt_id, .. }) => *attempt_id,
        other => panic!("the third projection is the running D-7 view: {other:?}"),
    };
    let expected = vec![
        ToolProjection::ApprovalStatus {
            approval_id,
            attempt_id,
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        },
        ToolProjection::ApprovalStatus {
            approval_id,
            attempt_id,
            status: ApprovalStatus::Accepted,
            decision: Some(ApprovalDecision::Accepted),
            version: 2,
        },
        ToolProjection::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        },
        ToolProjection::ToolResult {
            attempt_id,
            status: koduck_ai::domain::execution::ExecutionStatus::Succeeded,
            code: None,
            effect_state: EffectState::Started,
            output_bytes: 2,
            output_digest: Some(koduck_ai::application::output_digest(b"ok")),
            version: 3,
        },
    ];
    assert_eq!(
        appended, expected,
        "the canonical projection sequence is ordered"
    );
    assert_eq!(
        projections.events.len(),
        8,
        "each projection appends once and publishes once"
    );
    for pair in projections.events.chunks(2) {
        assert_eq!(
            pair[0].0,
            ProjectionPhase::Append,
            "append precedes publish"
        );
        assert_eq!(
            pair[1].0,
            ProjectionPhase::Publish,
            "publish follows append"
        );
        assert_eq!(
            pair[0].1, pair[1].1,
            "the published value is the appended value"
        );
    }

    // Deleting the projections changes no canonical state: the same call
    // without any projection sink reaches the identical terminal with the
    // same single dispatch and approval count.
    let mut decisions_without = 0;
    let mut unprojected_decision = |_request: &ApprovalRequest| {
        decisions_without += 1;
        (ApprovalDecision::Accepted, 2_000)
    };
    let inputs = ToolCallInputs {
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        ..inputs.clone()
    };
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(FixtureConfiguration {
            descriptor,
            profile,
        }),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute(
        &mut preparer,
        &mut coordinator,
        &inputs,
        &trust,
        &mut unprojected_decision,
        &mut || 1_000,
    )
    .expect("the unprojected call completes");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(coordinator.executor().calls, 1);
    assert_eq!(decisions_without, 1);

    // Replaying or forging projections changes no canonical state: feeding the
    // recorded sequence and a forged terminal approval view back through the
    // sink causes zero additional dispatches and zero additional approvals.
    let dispatches_before = coordinator.executor().calls;
    let mut replay_sink = RecordingProjections::default();
    for (_, projection) in projections
        .events
        .iter()
        .filter(|(phase, _)| *phase == ProjectionPhase::Append)
    {
        replay_sink
            .append(projection)
            .expect("replay append records");
        replay_sink.publish(projection);
    }
    let forged = ToolProjection::ApprovalStatus {
        approval_id: ApprovalId::new(),
        attempt_id: AttemptId::new(),
        status: ApprovalStatus::Accepted,
        decision: Some(ApprovalDecision::Accepted),
        version: 9,
    };
    let mut sink = RecordingProjections::default();
    let _ = sink.append(&forged);
    sink.publish(&forged);
    assert_eq!(
        coordinator.executor().calls,
        dispatches_before,
        "replayed or forged projections cause zero additional dispatches"
    );
    assert_eq!(decisions, 1, "no additional D-6 was created");
    assert_eq!(
        coordinator.committer().calls,
        1,
        "no additional terminal was committed"
    );
}

/// Shared approval-required driver fixture for the projection regression
/// tests below: one active descriptor, one allowing profile, scoped trust,
/// and a 600-second Turn deadline bounding the 5-minute D-6 window.
struct DriverFixture {
    configuration: FixtureConfiguration,
    inputs: ToolCallInputs,
    trust: TrustContext,
}

fn driver_fixture() -> DriverFixture {
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
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
    let trust = TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "requester",
    )
    .expect("valid principal")
    .with_approval_scopes(ApprovalScopes::from_validated([
        koduck_ai::application::TOOL_APPROVAL_SCOPE,
    ]));
    DriverFixture {
        configuration: FixtureConfiguration {
            descriptor,
            profile,
        },
        inputs: ToolCallInputs {
            tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            lease_generation: LeaseGeneration::initial(),
            profile_id: String::from("profile-default"),
            profile_version: String::from("v1"),
            action: Action::new(
                "fixture.tool",
                "v1",
                Effect::ExternalWrite,
                "fixture-target",
                parse_action_parameters("{}").expect("valid parameters"),
            )
            .expect("valid action"),
            turn_deadline_millis: 600_000,
        },
        trust,
    }
}

fn appended_projections(projections: &RecordingProjections) -> Vec<ToolProjection> {
    projections
        .events
        .iter()
        .filter(|(phase, _)| *phase == ProjectionPhase::Append)
        .map(|(_, projection)| projection.clone())
        .collect()
}

/// Lease validator that fences every check.
struct FencedLease;
impl LeaseValidator for FencedLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Fenced
    }
}

/// Lease that plays a fixed sequence of decisions so one check can be fenced
/// at an exact point in the coordinator sequence.
struct SequencedLease {
    decisions: std::collections::VecDeque<bool>,
}

impl LeaseValidator for SequencedLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        if self.decisions.pop_front().unwrap_or(false) {
            LeaseCheck::Current
        } else {
            LeaseCheck::Fenced
        }
    }
}

/// Committer that replays an already persisted canonical terminal at one
/// configured version, as a lost-acknowledgement or competing writer would.
struct ExistingCommitter {
    version: u64,
}

impl AttemptCommitter for ExistingCommitter {
    fn commit_outcome(
        &mut self,
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Existing(Box::new(
            CanonicalAttemptTerminal::from_persistence(
                binding.clone(),
                self.version,
                outcome.clone(),
            )
            .expect("fixture terminal is valid"),
        )))
    }
}

/// D-3 sink whose durable append is never available.
#[derive(Default)]
struct UnavailableProjections {
    publishes: usize,
}

impl ToolProjectionSink for UnavailableProjections {
    fn append(&mut self, _projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        Err(ToolProjectionError::Unavailable)
    }

    fn publish(&mut self, _projection: &ToolProjection) {
        self.publishes += 1;
    }
}

#[test]
fn requested_approval_is_projected_only_after_preparation_succeeds() {
    let fixture = driver_fixture();
    let mut projections = RecordingProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(FencedLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        FencedLease,
        WinningCommitter { calls: 0 },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 2_000);

    let result = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    );

    assert!(
        matches!(
            result,
            Err(ToolCallError::Preparation(
                ExecutionPreparationError::OwnerFenced
            ))
        ),
        "a fenced owner rejects preparation: {result:?}"
    );
    assert!(
        projections.events.is_empty(),
        "a rejected preparation leaves no unresolvable pending approval projection"
    );
}

#[test]
fn late_approval_decision_projects_the_expired_terminal() {
    let fixture = driver_fixture();
    let mut projections = RecordingProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        WinningCommitter { calls: 0 },
    );
    // The D-6 window opened at 1,000ms and expires at 301,000ms; this
    // decision arrives at 400,000ms and terminalizes the record as expired.
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 400_000);

    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    )
    .expect("a late decision still reaches a terminal outcome");
    assert!(
        matches!(outcome, ToolExecutionOutcome::Cancelled { .. }),
        "a decision after the D-6 expiry cancels the prepared attempt"
    );

    let appended = appended_projections(&projections);
    let (approval_id, attempt_id) = match (appended.first(), appended.last()) {
        (
            Some(ToolProjection::ApprovalStatus { approval_id, .. }),
            Some(ToolProjection::ToolResult { attempt_id, .. }),
        ) => (*approval_id, *attempt_id),
        other => panic!("the sequence opens with the D-6 and closes with the D-7: {other:?}"),
    };
    assert_eq!(
        appended,
        vec![
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: ApprovalStatus::Expired,
                decision: None,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id,
                status: ExecutionStatus::Cancelled,
                code: None,
                effect_state: EffectState::NotStarted,
                output_bytes: 0,
                output_digest: None,
                version: 3,
            },
        ],
        "the expired D-6 terminal is projected before the cancelled tool result"
    );
}

#[test]
fn running_projection_survives_a_post_claim_fence() {
    let fixture = driver_fixture();
    let mut projections = RecordingProjections::default();
    // The preparation check is current, the pre-dispatch check is current,
    // and the post-claim check fences the owner after the canonical running
    // transition already won.
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(SequencedLease {
                decisions: [true].into(),
            });
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        SequencedLease {
            decisions: [true, false].into(),
        },
        WinningCommitter { calls: 0 },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 2_000);

    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    )
    .expect("a post-claim fence closes the attempt as cancelled");
    assert!(
        matches!(outcome, ToolExecutionOutcome::Cancelled { .. }),
        "a post-claim fence cancels the claimed attempt without dispatch"
    );

    let appended = appended_projections(&projections);
    let (approval_id, attempt_id) = match (appended.first(), appended.last()) {
        (
            Some(ToolProjection::ApprovalStatus { approval_id, .. }),
            Some(ToolProjection::ToolResult { attempt_id, .. }),
        ) => (*approval_id, *attempt_id),
        other => panic!("the sequence opens with the D-6 and closes with the D-7: {other:?}"),
    };
    assert_eq!(
        appended,
        vec![
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: ApprovalStatus::Requested,
                decision: None,
                version: 1,
            },
            ToolProjection::ApprovalStatus {
                approval_id,
                attempt_id,
                status: ApprovalStatus::Accepted,
                decision: Some(ApprovalDecision::Accepted),
                version: 2,
            },
            ToolProjection::ToolCall {
                descriptor_id: "fixture.tool".to_owned(),
                descriptor_version: "v1".to_owned(),
                target: "fixture-target".to_owned(),
                attempt_id,
                status: ExecutionStatus::Running,
                version: 2,
            },
            ToolProjection::ToolResult {
                attempt_id,
                status: ExecutionStatus::Cancelled,
                code: None,
                effect_state: EffectState::NotStarted,
                output_bytes: 0,
                output_digest: None,
                version: 3,
            },
        ],
        "the canonical running transition is projected before the cancelled terminal"
    );
}

#[test]
fn replayed_terminal_must_carry_the_canonical_transition_version() {
    // A consistent replay returns the persisted terminal, and its projection
    // carries the same canonical terminal version.
    let fixture = driver_fixture();
    let mut projections = RecordingProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        ExistingCommitter { version: 3 },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 2_000);
    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    )
    .expect("a consistent replayed terminal is returned");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    let terminal = appended_projections(&projections)
        .into_iter()
        .last()
        .expect("a terminal result projection exists");
    assert_eq!(
        terminal,
        ToolProjection::ToolResult {
            attempt_id: match terminal {
                ToolProjection::ToolResult { attempt_id, .. } => attempt_id,
                _ => unreachable!("the last projection is the terminal result"),
            },
            status: ExecutionStatus::Succeeded,
            code: None,
            effect_state: EffectState::Started,
            output_bytes: 2,
            output_digest: Some(koduck_ai::application::output_digest(b"ok")),
            version: 3,
        },
        "the replayed terminal projects at the canonical transition version"
    );

    // A replayed terminal whose persisted version contradicts the canonical
    // D-7 transition version is a conflict: projecting it would fabricate a
    // canonical version, so reconciliation owns the next transition instead.
    let fixture = driver_fixture();
    let mut projections = RecordingProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        ExistingCommitter { version: 4 },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 2_000);
    let result = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    );
    assert!(
        matches!(
            result,
            Err(ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    ..
                }
            ))
        ),
        "a version-inconsistent replayed terminal is a conflict: {result:?}"
    );
}

#[test]
fn unavailable_projection_append_suppresses_publish_without_changing_the_outcome() {
    let fixture = driver_fixture();
    let mut projections = UnavailableProjections::default();
    let mut preparer =
        ToolExecutionRuntime::new(&koduck_ai::application::ToolExecutionAuthorityRoot::new())
            .preparer(CurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        OneShotExecutor { calls: 0 },
        CurrentLease,
        WinningCommitter { calls: 0 },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Accepted, 2_000);

    let outcome = ToolExecutionDriver::new(
        ToolAuthorizationService::new(fixture.configuration),
        koduck_ai::application::ApprovalDecisionService::new(AllowApprovals),
    )
    .execute_projected(
        &mut preparer,
        &mut coordinator,
        &fixture.inputs,
        &fixture.trust,
        &mut decision,
        &mut || 1_000,
        &mut projections,
    )
    .expect("a failed projection append never blocks the canonical outcome");

    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(
        projections.publishes, 0,
        "nothing is published without a durable append"
    );
    assert_eq!(coordinator.executor().calls, 1);
    assert_eq!(coordinator.committer().calls, 1);
}
