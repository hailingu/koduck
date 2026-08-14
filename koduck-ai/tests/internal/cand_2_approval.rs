// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ApprovalAuthorizer, ApprovalDecisionService, ExecutionPreparationError, ExecutionPreparer,
    LeaseCheck, LeaseValidator, ToolAuthorizationService, ToolExecutionAuthorityRoot,
    ToolExecutionRuntime, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ApprovalStatus, AttemptId,
    ExactActionBinding, ExecutionAttempt, ExecutionError, ExecutionStatus, TurnExecutionAuthority,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};
use uuid::Uuid;

fn exact_binding(attempt_id: AttemptId) -> ExactActionBinding {
    exact_binding_for(ThreadId::new(), TurnId::new(), attempt_id)
}

fn exact_binding_for(
    thread_id: ThreadId,
    turn_id: TurnId,
    attempt_id: AttemptId,
) -> ExactActionBinding {
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        attempt_id,
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
    authorize_for_profile(binding, "profile-default")
}

fn authorize_for_profile(binding: ExactActionBinding, profile_id: &str) -> ExactActionBinding {
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
    let profile = PermissionProfile::builder(profile_id, "v1")
        .expect("valid profile")
        .allow(
            "fixture.tool",
            "v1",
            Effect::ExternalWrite,
            "fixture-target",
        )
        .expect("valid profile entry")
        .build();
    ToolAuthorizationService::new(FixturePolicyConfiguration {
        descriptor,
        profile,
    })
    .authorize_binding(binding)
    .expect("fixture binding is policy-authorized")
}

struct FixturePolicyConfiguration {
    descriptor: CapabilityDescriptor,
    profile: PermissionProfile,
}

impl ToolPolicyConfiguration for FixturePolicyConfiguration {
    fn descriptor_for(&self, _action: &Action) -> Option<&CapabilityDescriptor> {
        Some(&self.descriptor)
    }

    fn profile_for(&self, profile_id: &str, profile_version: &str) -> Option<&PermissionProfile> {
        (self.profile.id() == profile_id && self.profile.version() == profile_version)
            .then_some(&self.profile)
    }
}

struct FixtureApprovalAuthorizer {
    allow: bool,
}

impl ApprovalAuthorizer for FixtureApprovalAuthorizer {
    fn can_resolve_tool_approval(
        &mut self,
        _binding: &ExactActionBinding,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> bool {
        self.allow
    }
}

fn resolve(
    approval: &mut ApprovalRequest,
    decision: ApprovalDecision,
    decided_at_millis: u64,
) -> Result<u64, ApprovalError> {
    let thread_id = approval.thread_id();
    let trust = TrustContext::new(approval.tenant_id().clone(), "approver-a")
        .expect("valid authenticated principal");
    ApprovalDecisionService::new(FixtureApprovalAuthorizer { allow: true }).resolve(
        approval,
        &trust,
        thread_id,
        decision,
        decided_at_millis,
    )
}

fn new_preparer() -> ExecutionPreparer<CurrentLease> {
    ToolExecutionRuntime::new(&ToolExecutionAuthorityRoot::new()).preparer(CurrentLease)
}

struct CurrentLease;

impl LeaseValidator for CurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

fn prepare(
    preparer: &mut ExecutionPreparer<CurrentLease>,
    binding: ExactActionBinding,
) -> Result<(TurnExecutionAuthority, ExecutionAttempt), ExecutionError> {
    match preparer.prepare(binding) {
        Ok(pair) => Ok(pair),
        Err(ExecutionPreparationError::Rejected(error)) => Err(error),
        Err(ExecutionPreparationError::OwnerFenced) => {
            panic!("the deterministic test lease is current")
        }
        Err(ExecutionPreparationError::LeaseUnavailable) => {
            panic!("the deterministic test lease never becomes unavailable")
        }
    }
}

#[test]
fn exact_approval_authorizes_one_attempt() {
    let attempt_id = AttemptId::new();
    let binding = exact_binding(attempt_id);
    let mut approval =
        ApprovalRequest::new(binding.clone(), 1_000, 600_000).expect("valid approval request");
    resolve(&mut approval, ApprovalDecision::Accepted, 2_000).expect("approval is accepted");
    assert_eq!(approval.status(), ApprovalStatus::Accepted);

    let mut preparer = new_preparer();
    let (mut authority, mut attempt) =
        prepare(&mut preparer, binding.clone()).expect("attempt slot available");
    assert_eq!(authority.used(), 1);
    authority
        .claim_dispatch(&mut attempt, Some(&approval), 2_001)
        .expect("exact approval starts the bound attempt");
    assert_eq!(attempt.status(), ExecutionStatus::Running);
    assert_eq!(
        authority.claim_dispatch(&mut attempt, Some(&approval), 2_002),
        Err(ExecutionError::AlreadyDispatched)
    );

    let drifted = exact_binding(attempt_id);
    let mut drifted_preparer = new_preparer();
    let (mut drifted_authority, mut drifted_attempt) =
        prepare(&mut drifted_preparer, drifted).expect("another Turn may prepare its own attempt");
    assert_eq!(
        drifted_authority.claim_dispatch(&mut drifted_attempt, Some(&approval), 2_003),
        Err(ExecutionError::ApprovalMismatch)
    );
}

#[test]
fn approval_resolution_is_conditional_and_idempotent() {
    let binding = exact_binding(AttemptId::new());
    let mut approval =
        ApprovalRequest::new(binding, 1_000, 600_000).expect("valid approval request");

    let accepted_version =
        resolve(&mut approval, ApprovalDecision::Accepted, 2_000).expect("first decision wins");
    assert_eq!(accepted_version, 2);
    assert_eq!(
        resolve(&mut approval, ApprovalDecision::Accepted, 2_100)
            .expect("identical decision is idempotent"),
        accepted_version
    );
    assert_eq!(
        resolve(&mut approval, ApprovalDecision::Declined, 2_200),
        Err(ApprovalError::AlreadyResolved)
    );
}

#[test]
fn invalid_approval_scope_mutates_no_canonical_state() {
    let binding = exact_binding(AttemptId::new());
    let mut approval =
        ApprovalRequest::new(binding, 1_000, 600_000).expect("valid approval request");
    let original_version = approval.version();
    let wrong_tenant = TrustContext::new(
        TenantId::new("tenant-b").expect("valid tenant"),
        "approver-a",
    )
    .expect("valid principal");
    let thread_id = approval.thread_id();

    assert_eq!(
        ApprovalDecisionService::new(FixtureApprovalAuthorizer { allow: true }).resolve(
            &mut approval,
            &wrong_tenant,
            thread_id,
            ApprovalDecision::Accepted,
            2_000,
        ),
        Err(ApprovalError::NotAuthorized)
    );
    assert_eq!(approval.status(), ApprovalStatus::Requested);
    assert_eq!(approval.version(), original_version);
}

#[test]
fn caller_asserted_approval_scope_is_not_c7_authority() {
    let binding = exact_binding(AttemptId::new());
    let mut approval =
        ApprovalRequest::new(binding, 1_000, 600_000).expect("valid approval request");
    let asserted = TrustContext::new(approval.tenant_id().clone(), "unverified-caller")
        .expect("syntactically valid identity");
    let thread_id = approval.thread_id();

    assert_eq!(
        ApprovalDecisionService::new(FixtureApprovalAuthorizer { allow: false }).resolve(
            &mut approval,
            &asserted,
            thread_id,
            ApprovalDecision::Accepted,
            2_000,
        ),
        Err(ApprovalError::NotAuthorized)
    );
    assert_eq!(approval.status(), ApprovalStatus::Requested);
}

#[test]
fn identical_terminal_decision_remains_idempotent_after_expiry_time() {
    let binding = exact_binding(AttemptId::new());
    let mut approval =
        ApprovalRequest::new(binding, 1_000, 600_000).expect("valid approval request");
    let accepted_version =
        resolve(&mut approval, ApprovalDecision::Accepted, 2_000).expect("first decision wins");

    assert_eq!(
        resolve(&mut approval, ApprovalDecision::Accepted, 301_000)
            .expect("identical terminal replay remains idempotent"),
        accepted_version
    );
}

#[test]
fn retry_consumes_a_new_attempt_slot() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let mut preparer = new_preparer();
    for expected in 1..=16 {
        let (authority, _attempt) = prepare(
            &mut preparer,
            exact_binding_for(thread_id, turn_id, AttemptId::new()),
        )
        .expect("slot is available");
        assert_eq!(authority.used(), expected);
    }
    assert_eq!(
        prepare(
            &mut preparer,
            exact_binding_for(thread_id, turn_id, AttemptId::new(),),
        )
        .map(|_| ()),
        Err(ExecutionError::AttemptLimit)
    );
}

#[test]
fn duplicate_attempt_identity_cannot_allocate_twice() {
    let binding = exact_binding(AttemptId::new());
    let mut preparer = new_preparer();
    let (authority, _attempt) =
        prepare(&mut preparer, binding.clone()).expect("first allocation succeeds");

    assert_eq!(
        prepare(&mut preparer, binding).map(|_| ()),
        Err(ExecutionError::AttemptAlreadyAllocated)
    );
    assert_eq!(authority.used(), 1);
}

#[test]
fn reconstructed_turn_authority_cannot_reset_budget_or_attempt_identity() {
    let binding = exact_binding(AttemptId::new());
    let mut preparer = new_preparer();
    let (first, _attempt) =
        prepare(&mut preparer, binding.clone()).expect("first authority allocates the D-7");

    assert_eq!(
        prepare(&mut preparer, binding).map(|_| ()),
        Err(ExecutionError::AttemptAlreadyAllocated)
    );
    assert_eq!(first.used(), 1);
}

#[test]
fn reconstructed_turn_authority_survives_all_handle_drops() {
    let binding = exact_binding(AttemptId::new());
    let mut preparer = new_preparer();
    {
        let (first, first_attempt) =
            prepare(&mut preparer, binding.clone()).expect("first authority allocates the D-7");
        drop(first_attempt);
        drop(first);
    }

    assert_eq!(
        prepare(&mut preparer, binding).map(|_| ()),
        Err(ExecutionError::AttemptAlreadyAllocated)
    );
}

#[test]
fn one_turn_cannot_open_a_second_authority_with_another_profile() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let first = exact_binding_for(thread_id, turn_id, AttemptId::new());
    let drifted = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
        ("profile-escalated", "v1"),
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
    let drifted = authorize_for_profile(drifted, "profile-escalated");
    let mut preparer = new_preparer();
    let (authority, _attempt) = prepare(&mut preparer, first).expect("first profile wins");

    assert_eq!(
        prepare(&mut preparer, drifted).map(|_| ()),
        Err(ExecutionError::TurnMismatch)
    );
    assert_eq!(authority.used(), 1);
}

#[test]
fn turn_authority_allows_only_one_running_attempt() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let first_binding = exact_binding_for(thread_id, turn_id, AttemptId::new());
    let second_binding = exact_binding_for(thread_id, turn_id, AttemptId::new());
    let mut first_approval =
        ApprovalRequest::new(first_binding.clone(), 1_000, 600_000).expect("valid approval");
    resolve(&mut first_approval, ApprovalDecision::Accepted, 2_000).expect("accepted");
    let mut second_approval =
        ApprovalRequest::new(second_binding.clone(), 1_000, 600_000).expect("valid approval");
    resolve(&mut second_approval, ApprovalDecision::Accepted, 2_000).expect("accepted");
    let mut preparer = new_preparer();
    let (mut authority, mut first) = prepare(&mut preparer, first_binding).expect("first prepared");
    let (_second_authority, mut second) =
        prepare(&mut preparer, second_binding).expect("second prepared");

    authority
        .claim_dispatch(&mut first, Some(&first_approval), 2_001)
        .expect("first attempt claims the Turn running slot");
    assert_eq!(
        authority.claim_dispatch(&mut second, Some(&second_approval), 2_002),
        Err(ExecutionError::ConcurrentAttempt)
    );
    assert_eq!(second.status(), ExecutionStatus::Prepared);
}

#[test]
fn action_digest_has_stable_sha256_width() {
    let binding = exact_binding(AttemptId::new());
    let digest = format!("{:x}", binding.action_digest());

    assert_eq!(digest.len(), 64);
}

#[test]
fn action_digest_matches_the_canonical_fixture() {
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::from_uuid(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()),
        TurnId::from_uuid(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::from_uuid(Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()),
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

    assert_eq!(
        format!("{:x}", binding.action_digest()),
        "36808ae17a965916bf5e3d5795a7ad468ba3b01c470c9b9810fade25f7f67bc1"
    );
}

#[test]
fn approval_binding_distinguishes_profile_identity_from_version() {
    let attempt_id = AttemptId::new();
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let action = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters(r#"{"value":1}"#).expect("valid parameters"),
    )
    .expect("valid action");
    let original = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
        ("profile-a", "v1"),
        attempt_id,
        action.clone(),
    )
    .expect("valid binding");
    let original = authorize_for_profile(original, "profile-a");
    let drifted_profile = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        thread_id,
        turn_id,
        LeaseGeneration::initial(),
        ("profile-b", "v1"),
        attempt_id,
        action,
    )
    .expect("valid binding");
    let drifted_profile = authorize_for_profile(drifted_profile, "profile-b");
    let mut approval =
        ApprovalRequest::new(original, 1_000, 600_000).expect("valid approval request");
    resolve(&mut approval, ApprovalDecision::Accepted, 2_000).expect("accepted");

    assert_eq!(
        approval.authorize(&drifted_profile),
        Err(ApprovalError::BindingMismatch)
    );
}

#[test]
fn approval_expires_at_the_earlier_deadline() {
    let later_turn = ApprovalRequest::new(exact_binding(AttemptId::new()), 1_000, 900_000)
        .expect("five-minute window exists");
    assert_eq!(later_turn.expires_at_millis(), 301_000);

    let earlier_turn = ApprovalRequest::new(exact_binding(AttemptId::new()), 1_000, 121_000)
        .expect("two-minute Turn window exists");
    assert_eq!(earlier_turn.expires_at_millis(), 121_000);
}
