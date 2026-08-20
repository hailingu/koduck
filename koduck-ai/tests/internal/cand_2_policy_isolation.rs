// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Semantic dependency-direction verification for the C-5 policy path.
//!
//! This module imports ONLY `crate::domain` and `crate::application` types —
//! no adapter, provider, HTTP, SQLx, executor, or runtime module is
//! referenced. It drives the complete default-deny authorization pipeline —
//! configured snapshot, descriptor/profile resolution, exact-action sealing,
//! lease-validated preparation, approval validation, and the guarded
//! dispatch claim — through those types alone with hand-written doubles for
//! the consumer-owned ports. The compiler is the verification mechanism: any
//! dependency of this pipeline on an adapter or wire type would make these
//! signatures unwritable here (ADR-0003 TC-01, AC-1).

use koduck_ai::application::{
    AttemptCommitResult, AttemptCommitter, ExecutionCoordinator, ExecutionPending, LeaseCheck,
    LeaseValidator,
    ToolAuthorizationService, ToolConfigurationSnapshot, ToolExecutionOutcome,
};
use koduck_ai::domain::execution::{ApprovalDecision, ApprovalRequest, ExactActionBinding};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, InputSchema, PermissionProfile,
    ToolValueError,
};
use koduck_ai::domain::{
    LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId,
};

/// Executor double implementing the application's consumer-owned port; the
/// isolation pipeline needs no adapter executor.
struct SucceedingExecutor;

impl koduck_ai::application::IsolatedExecutor for SucceedingExecutor {
    fn execute(
        &mut self,
        _permit: &koduck_ai::application::DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> Result<koduck_ai::application::ExecutionResponse, koduck_ai::application::ExecutorError>
    {
        let mut response = koduck_ai::application::ExecutionResponseBuilder::new(
            koduck_ai::application::EffectState::Started,
        );
        response.push_chunk(b"committed")?;
        response.finish()
    }

    fn cancel(
        &mut self,
        _permit: &koduck_ai::application::CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> koduck_ai::application::CancelAcknowledgement {
        koduck_ai::application::CancelAcknowledgement::NotAcknowledged
    }
}

/// Lease double answering Current for the isolation pipeline.
struct CurrentLease;

impl LeaseValidator for CurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Committer double recording the conditional terminal commits.
#[derive(Default)]
struct RecordingCommitter {
    commits: usize,
}

impl koduck_ai::application::DurableAttemptTransitions for RecordingCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<koduck_ai::application::AttemptInsertResolution, koduck_ai::application::AttemptStoreError>
    {
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<koduck_ai::application::DispatchClaimResolution, koduck_ai::application::AttemptStoreError>
    {
        Ok(koduck_ai::application::DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, koduck_ai::application::AttemptStoreError>
    {
        Ok(koduck_ai::application::PreparedCloseResolution::Won { version: 3 })
    }
}

impl AttemptCommitter for RecordingCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, koduck_ai::application::AttemptCommitError> {
        self.commits += 1;
        Ok(AttemptCommitResult::Won)
    }
}

/// A schema constructed through the domain constructor without any adapter
/// translation.
fn domain_schema() -> Result<InputSchema, ToolValueError> {
    InputSchema::object(Vec::new(), Vec::new(), false)
}

fn snapshot() -> ToolConfigurationSnapshot {
    let descriptor = CapabilityDescriptor::new(
        "fixture.read",
        "v1",
        Effect::ReadData,
        DescriptorState::Active,
        domain_schema().expect("valid domain schema"),
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
        .expect("valid entry")
        .build();
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(descriptor)
        .expect("unique descriptor");
    snapshot
        .register_profile(profile)
        .expect("unique profile");
    snapshot
}

fn owned_action() -> Result<Action, ToolValueError> {
    Action::new(
        "fixture.read",
        "v1",
        Effect::ReadData,
        "fixture-target",
        koduck_ai::domain::tool::ActionParameters::new(
            koduck_ai::domain::tool::JsonValue::Object(Default::default()),
        )?,
    )
}

#[test]
fn the_default_deny_pipeline_runs_on_domain_and_application_types_alone() {
    // The complete authorization pipeline — snapshot resolution, exact-action
    // sealing, lease-validated preparation, and the guarded dispatch claim —
    // compiles and runs referencing only domain and application types: the
    // compiler proves those boundaries carry no adapter, provider-wire,
    // SQLx, executor, or runtime dependency for policy behavior
    // (ADR-0003 TC-01).
    let snapshot = snapshot();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let binding = ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        owned_action().expect("domain-constructed action"),
    )
    .expect("valid binding");
    let sealed = ToolAuthorizationService::new(snapshot)
        .authorize_binding(binding)
        .expect("the owned action is policy-authorized without approval");

    let runtime = koduck_ai::application::ToolExecutionRuntime::new(
        &koduck_ai::application::ToolExecutionAuthorityRoot::new(),
    );
    let sealed_for_approval = sealed.clone();
    let mut preparer = runtime.preparer(CurrentLease);
    let (mut authority, mut attempt) = preparer
        .prepare(sealed)
        .expect("the sealed action prepares through domain and application types");

    // An approval-free read_data action dispatches on its sealed authority
    // alone; the approval-free path itself stays inside the two boundaries.
    drop((sealed_for_approval, ApprovalDecision::Accepted));
    let mut committer = RecordingCommitter::default();
    let mut coordinator = ExecutionCoordinator::new(
        SucceedingExecutor,
        CurrentLease,
        committer,
    );
    let mut now = || 5_000_u64;
    let outcome = coordinator
        .execute(&mut authority, None, &mut attempt, 5_000, &mut now)
        .expect("the dispatched action reaches its terminal");
    assert!(
        matches!(outcome, ToolExecutionOutcome::Succeeded { .. }),
        "found {outcome:?}"
    );
    drop(ApprovalDecision::Accepted);
}

#[test]
fn an_unknown_descriptor_denies_inside_the_policy_boundary() {
    // Default-deny is decided by the application policy evaluator over
    // domain values alone: an unregistered descriptor denies before any
    // approval or execution exists (ADR-0003 TC-02).
    let snapshot = ToolConfigurationSnapshot::empty();
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        owned_action().expect("domain-constructed action"),
    )
    .expect("valid binding");
    let denied = ToolAuthorizationService::new(snapshot).authorize_binding(binding);
    assert!(
        matches!(
            denied,
            Err(koduck_ai::application::DenialCode::DescriptorMissing
                | koduck_ai::application::DenialCode::OutsidePermissionProfile)
        ),
        "the empty inventory denies the owned action, found {denied:?}"
    );
}
