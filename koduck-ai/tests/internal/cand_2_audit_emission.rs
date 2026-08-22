// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Production audit-emission legs of the AC-13 harness: every default-deny
//! policy terminal, canonical D-6 resolution, D-7 execution terminal, and
//! interruption close emits exactly one correlated, bounded record through
//! the C-5 boundary (ADR-0003 TC-14).

use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::attempt_store::{
    AttemptInsertResolution, AttemptStoreError, DispatchClaimResolution,
};
use koduck_ai::application::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, CancelAcknowledgement, CancelPermit,
    DispatchPermit, DurableAttemptTransitions, EffectState, ExecutionAttemptInterruptionGuard,
    ExecutionAttemptLiveness, ExecutionCoordinator, ExecutionPending, ExecutionResponse,
    ExecutionResponseBuilder, ExecutorError, IsolatedExecutor, LeaseCheck, LeaseValidator,
    MAX_AUDIT_RECORD_BYTES, ModelToolCall, ModelToolResult, NoToolProjections, TOOL_APPROVAL_SCOPE,
    ToolCallExecutor, ToolCallInputs, ToolCallTurnContext, ToolConfigurationSnapshot,
    ToolExecutionAssembly, ToolExecutionOutcome, ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalRequest, AttemptId, ExactActionBinding,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};
use koduck_ai::runtime::tool_executor::BoundaryToolCallExecutor;

/// Parses one serialized audit record into its JSON fields.
fn recorded_json_fields(serialized: &str) -> serde_json::Value {
    serde_json::from_str(serialized).expect("an audit record is valid JSON")
}

/// Collects the `policy_decision` field of every recorded audit, in order.
fn recorded_policy_decisions(audits: &RecordingAudits) -> Vec<String> {
    audits
        .serialized()
        .iter()
        .map(|serialized| {
            let fields = recorded_json_fields(serialized);
            fields["policy_decision"]
                .as_str()
                .expect("policy_decision is a string")
                .to_owned()
        })
        .collect()
}

/// Recording audit trail capturing every serialized terminal record.
///
/// The buffer is shared between clones, so a trail injected into the
/// production executor and the harness copy observe the same records.
#[derive(Clone, Default)]
struct RecordingAudits {
    serialized: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl RecordingAudits {
    /// Returns every serialized record captured so far, in order.
    fn serialized(&self) -> Vec<String> {
        self.serialized
            .lock()
            .expect("audit log is healthy")
            .clone()
    }
}

impl koduck_ai::application::ToolAuditTrail for RecordingAudits {
    fn emit(
        &mut self,
        record: &koduck_ai::application::ToolAuditRecord,
    ) -> Result<(), koduck_ai::application::ToolAuditEmitError> {
        let serialized = koduck_ai::adapters::audit::serialize_audit_record(record)
            .map_err(koduck_ai::application::ToolAuditEmitError::TooLarge)?;
        self.serialized
            .lock()
            .expect("audit log is healthy")
            .push(serialized);
        Ok(())
    }
}

/// Winning process-local committer double for these legs.
#[derive(Clone, Copy, Default)]
struct WinningCommitter;

impl AttemptCommitter for WinningCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Won)
    }
}

impl DurableAttemptTransitions for WinningCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Ok(AttemptInsertResolution::Inserted)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        Ok(koduck_ai::application::PreparedCloseResolution::Won { version: 3 })
    }
}

impl ExecutionAttemptInterruptionGuard for WinningCommitter {
    fn begin_interruption(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<koduck_ai::application::InterruptionBarrierResolution, AttemptStoreError> {
        Ok(koduck_ai::application::InterruptionBarrierResolution::Established)
    }
}

impl ExecutionAttemptLiveness for WinningCommitter {
    fn has_live_attempt(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        Ok(false)
    }
}

/// Always-current lease double: these legs prove emission, not fencing.
#[derive(Clone, Copy)]
struct AlwaysCurrentLease;

impl LeaseValidator for AlwaysCurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Executor double returning one bounded success.
#[derive(Clone, Copy, Default)]
struct SucceedingExecutor;

impl IsolatedExecutor for SucceedingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        let mut response = ExecutionResponseBuilder::new(EffectState::Started);
        response
            .push_chunk(b"committed")
            .expect("fixture output is bounded");
        response.finish()
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}

fn snapshot(effect: Effect) -> ToolConfigurationSnapshot {
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(
            CapabilityDescriptor::new(
                "fixture.tool",
                "v1",
                effect,
                DescriptorState::Active,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid fixture schema"),
            )
            .expect("valid fixture descriptor"),
        )
        .expect("descriptor registers");
    snapshot
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.tool", "v1", effect, "fixture-target")
                .expect("valid profile entry")
                .build(),
        )
        .expect("profile registers");
    snapshot
}

fn inputs(effect: Effect, turn_deadline_millis: u64) -> ToolCallInputs {
    ToolCallInputs {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            effect,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis,
    }
}

fn scoped_trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid principal")
    .with_approval_scopes(koduck_ai::domain::ApprovalScopes::from_validated([
        TOOL_APPROVAL_SCOPE,
    ]))
}

/// Durable-transition double that accepts the initial prepared insert and
/// rejects the retry's insert with the typed attempt limit, driving the
/// record-stage exhaustion branch of the TC-08 retry.
#[derive(Clone, Default)]
struct BudgetLimitedCommitter {
    inserts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl AttemptCommitter for BudgetLimitedCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Won)
    }
}

impl DurableAttemptTransitions for BudgetLimitedCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        if self
            .inserts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            return Ok(AttemptInsertResolution::Inserted);
        }
        Err(AttemptStoreError::AttemptLimit)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        Ok(koduck_ai::application::PreparedCloseResolution::Won { version: 3 })
    }
}

/// Executor double failing pre-effect on every response, so the driver's
/// TC-08 retry runs into whatever budget state the fixture prepared.
#[derive(Clone, Copy, Default)]
struct PreEffectFailingExecutor;

impl IsolatedExecutor for PreEffectFailingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        Err(ExecutorError::new(
            koduck_ai::application::ExecutionFailure::ExecutorUnavailable,
            EffectState::NotStarted,
        ))
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: koduck_ai::application::ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}

/// Turn context for the production-executor legs.
fn turn_context() -> ToolCallTurnContext {
    ToolCallTurnContext {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
    }
}

/// Drives one model Tool call through the actual production executor with a
/// recording trail injected alongside the durable-transition doubles, so the
/// executor's own pre-driver denial path is observed (ADR-0003 TC-14).
fn drive_executor(
    configuration: ToolConfigurationSnapshot,
    name: &str,
    arguments: &str,
) -> (ModelToolResult, RecordingAudits) {
    let audits = RecordingAudits::default();
    let root = ToolExecutionRuntimeRoot::issue();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        configuration,
        WinningCommitter,
        AlwaysCurrentLease,
        audits.clone(),
        koduck_ai::application::NoCanonicalTurnTerminal,
    );
    let result = executor
        .execute_tool_call(
            ModelToolCall {
                name: name.to_owned(),
                arguments: arguments.to_owned(),
            },
            &turn_context(),
            &scoped_trust(),
            &mut NoToolProjections,
        )
        .expect("a typed denial is a recorded tool result, never a turn failure");
    (result, audits)
}

#[test]
fn pre_driver_policy_denials_emit_through_the_production_executor() {
    // descriptor_missing: the empty production inventory denies by name
    // before any trusted descriptor or profile exists, so the correlated
    // record carries the Turn identity with no descriptor or profile
    // metadata and no attempt or approval identity (TC-02/TC-14).
    let (result, audits) =
        drive_executor(ToolConfigurationSnapshot::empty(), "unresolved.tool", "{}");
    assert!(result.is_error);
    assert_eq!(result.content, "descriptor_missing");
    let records = audits.serialized();
    assert_eq!(
        records.len(),
        1,
        "one correlated record per pre-driver denial"
    );
    let fields = recorded_json_fields(&records[0]);
    assert_eq!(fields["policy_decision"], "descriptor_missing");
    assert!(fields["attempt_id"].is_null() && fields["approval_id"].is_null());
    assert_eq!(fields["tenant_id"], "tenant-a");
    assert!(fields["thread_id"].is_string() && fields["turn_id"].is_string());
    assert!(fields["descriptor_id"].is_null() && fields["profile_id"].is_null());

    // invalid_input: a resolved descriptor with unparseable arguments denies
    // before any D-6 or D-7 exists, carrying the resolved descriptor and the
    // selected Permission Profile without any exact-action digest.
    let (result, audits) = drive_executor(snapshot(Effect::ReadData), "fixture.tool", "not json");
    assert!(result.is_error);
    assert_eq!(result.content, "invalid_input");
    let records = audits.serialized();
    assert_eq!(records.len(), 1);
    let fields = recorded_json_fields(&records[0]);
    assert_eq!(fields["policy_decision"], "invalid_input");
    assert!(fields["attempt_id"].is_null() && fields["approval_id"].is_null());
    assert_eq!(fields["descriptor_id"], "fixture.tool");
    assert_eq!(fields["descriptor_version"], "v1");
    assert_eq!(fields["profile_id"], "profile-default");
    assert!(fields["action_digest"].is_null());

    // outside_permission_profile: the configured profile exists but does not
    // allow the resolved descriptor's exact capability tuple, so the denial
    // correlates the resolved descriptor against the denying profile.
    let mut outside = ToolConfigurationSnapshot::empty();
    outside
        .register_descriptor(
            CapabilityDescriptor::new(
                "fixture.tool",
                "v1",
                Effect::ReadData,
                DescriptorState::Active,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid fixture schema"),
            )
            .expect("valid fixture descriptor"),
        )
        .expect("descriptor registers");
    outside
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("other.tool", "v1", Effect::ReadData, "other-target")
                .expect("valid profile entry")
                .build(),
        )
        .expect("profile registers");
    let (result, audits) = drive_executor(outside, "fixture.tool", "{}");
    assert!(result.is_error);
    assert_eq!(result.content, "outside_permission_profile");
    let records = audits.serialized();
    assert_eq!(records.len(), 1);
    let fields = recorded_json_fields(&records[0]);
    assert_eq!(fields["policy_decision"], "outside_permission_profile");
    assert_eq!(fields["descriptor_id"], "fixture.tool");
    assert_eq!(fields["profile_id"], "profile-default");
    for serialized in &records {
        assert!(serialized.len() <= MAX_AUDIT_RECORD_BYTES);
    }
}

/// Drives one call through the production boundary with a recording sink.
fn drive(
    effect: Effect,
    turn_deadline_millis: u64,
    decision: impl FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
) -> (
    Result<ToolExecutionOutcome, koduck_ai::application::ToolCallError>,
    RecordingAudits,
) {
    let root = ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, snapshot(effect));
    let mut boundary = assembly.boundary(SucceedingExecutor, AlwaysCurrentLease, WinningCommitter);
    let mut audits = RecordingAudits::default();
    let mut projections = NoToolProjections;
    let outcome = boundary.execute_projected(
        &inputs(effect, turn_deadline_millis),
        &scoped_trust(),
        &mut {
            let mut decision = decision;
            move |request: &ApprovalRequest| decision(request)
        },
        &mut || 1_000,
        &mut projections,
        &mut audits,
    );
    (outcome, audits)
}

#[test]
fn policy_denial_emits_exactly_one_pre_attempt_record() {
    let root = ToolExecutionRuntimeRoot::issue();
    // An empty inventory denies every call by name before D-6 or D-7 exists.
    let assembly = ToolExecutionAssembly::new(&root, ToolConfigurationSnapshot::empty());
    let mut boundary = assembly.boundary(SucceedingExecutor, AlwaysCurrentLease, WinningCommitter);
    let mut audits = RecordingAudits::default();
    let mut projections = NoToolProjections;
    let error = boundary
        .execute_projected(
            &inputs(Effect::ReadData, u64::MAX),
            &scoped_trust(),
            &mut |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0),
            &mut || 1_000,
            &mut projections,
            &mut audits,
        )
        .expect_err("the unresolved descriptor denies");

    assert!(
        matches!(
            error,
            koduck_ai::application::ToolCallError::Denied(
                koduck_ai::application::DenialCode::OutsidePermissionProfile
            )
        ),
        "found {error:?}"
    );
    assert_eq!(
        audits.serialized().len(),
        1,
        "one record per denial terminal"
    );
    let fields = recorded_json_fields(&audits.serialized()[0]);
    assert_eq!(fields["policy_decision"], "outside_permission_profile");
    assert!(fields["attempt_id"].is_null() && fields["approval_id"].is_null());
    assert_eq!(fields["descriptor_id"], "fixture.tool");
}

#[test]
fn accepted_and_declined_approvals_emit_resolution_and_terminal_records() {
    // Accepted: one D-6 resolution record plus one executed(succeeded) record.
    let (outcome, audits) = drive(Effect::ExternalWrite, u64::MAX, |request| {
        (ApprovalDecision::Accepted, request.expires_at_millis() - 1)
    });
    assert!(matches!(
        outcome,
        Ok(ToolExecutionOutcome::Succeeded { .. })
    ));
    let decisions = recorded_policy_decisions(&audits);
    assert_eq!(
        decisions,
        vec!["approval_resolved".to_owned(), "executed".to_owned()],
        "found {decisions:?}"
    );
    let resolution = recorded_json_fields(&audits.serialized()[0]);
    assert_eq!(resolution["approval_status"], "accepted");
    assert_eq!(resolution["approval_decision"], "accepted");
    assert!(resolution["approval_id"].is_string());
    let terminal = recorded_json_fields(&audits.serialized()[1]);
    assert_eq!(terminal["execution_status"], "succeeded");
    assert_eq!(terminal["attempt_id"], resolution["attempt_id"]);

    // Declined: one D-6 resolution record plus one executed(cancelled) record.
    let (outcome, audits) = drive(Effect::ExternalWrite, u64::MAX, |_| {
        (ApprovalDecision::Declined, 2_000)
    });
    assert!(matches!(
        outcome,
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted
        })
    ));
    let decisions = recorded_policy_decisions(&audits);
    assert_eq!(
        decisions,
        vec!["approval_resolved".to_owned(), "executed".to_owned()]
    );
    let resolution = recorded_json_fields(&audits.serialized()[0]);
    assert_eq!(resolution["approval_decision"], "declined");
    let terminal = recorded_json_fields(&audits.serialized()[1]);
    assert_eq!(terminal["execution_status"], "cancelled");
}

#[test]
fn expired_d6_emits_the_expired_resolution_and_cancelled_terminal() {
    // A short Turn deadline expires the D-6 window before the decision, so
    // the canonical expired resolution and the cancelled D-7 terminal both
    // emit (TC-14).
    let (outcome, audits) = drive(Effect::ExternalWrite, 2_000, |request| {
        let _ = request;
        (ApprovalDecision::Accepted, 3_000)
    });
    assert!(matches!(
        outcome,
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted
        })
    ));
    let decisions = recorded_policy_decisions(&audits);
    assert_eq!(
        decisions,
        vec!["approval_resolved".to_owned(), "executed".to_owned()]
    );
    let resolution = recorded_json_fields(&audits.serialized()[0]);
    assert_eq!(resolution["approval_status"], "expired");
    assert!(resolution["approval_decision"].is_null());
    let terminal = recorded_json_fields(&audits.serialized()[1]);
    assert_eq!(terminal["execution_status"], "cancelled");
    for serialized in &audits.serialized() {
        assert!(serialized.len() <= MAX_AUDIT_RECORD_BYTES);
    }
}

#[test]
fn budget_exhausted_retry_terminal_emits_its_no_attempt_record() {
    // Attempt 16 fails pre-effect, so TC-08 permits one retry — but the Turn's
    // 16-slot budget is exhausted, and the delivered terminal is
    // failed/attempt_limit with no new D-7. Both the committed attempt-16
    // executor failure and the budget terminal itself must leave one
    // correlated audit record; the budget record carries the exact action
    // without any attempt or approval identity (ADR-0003 TC-08/TC-14).
    let configuration = snapshot(Effect::ReadData);
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let mut budget_inputs = inputs(Effect::ReadData, u64::MAX);
    budget_inputs.thread_id = thread_id;
    budget_inputs.turn_id = turn_id;
    let root = ToolExecutionRuntimeRoot::issue();
    // Pre-consume 15 of the 16 attempt slots on the same Turn authority, so
    // the initial call consumes slot 16 and the retry cannot allocate.
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let authorize = koduck_ai::application::ToolAuthorizationService::new(configuration.clone());
    for _ in 0..15 {
        let sealed = authorize
            .authorize_binding(
                ExactActionBinding::new(
                    TenantId::new("tenant-a").expect("valid tenant"),
                    thread_id,
                    turn_id,
                    LeaseGeneration::initial(),
                    ("profile-default", "v1"),
                    AttemptId::new(),
                    Action::new(
                        "fixture.tool",
                        "v1",
                        Effect::ReadData,
                        "fixture-target",
                        parse_action_parameters(r"{}").expect("valid parameters"),
                    )
                    .expect("valid action"),
                )
                .expect("valid binding"),
            )
            .expect("the pre-consumption binding is policy-authorized");
        preparer
            .prepare(sealed)
            .expect("a pre-consumption slot is available");
    }
    let mut boundary = ToolExecutionAssembly::new(&root, configuration).boundary(
        PreEffectFailingExecutor,
        AlwaysCurrentLease,
        WinningCommitter,
    );
    let mut audits = RecordingAudits::default();
    let outcome = boundary
        .execute_projected(
            &budget_inputs,
            &scoped_trust(),
            &mut |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0),
            &mut || 1_000,
            &mut NoToolProjections,
            &mut audits,
        )
        .expect("the budget-exhausted retry reaches a terminal outcome");

    assert!(matches!(
        outcome,
        ToolExecutionOutcome::Failed {
            code: koduck_ai::application::ExecutionFailure::AttemptLimit,
            effect_state: EffectState::NotStarted,
        }
    ));
    let records = audits.serialized();
    assert_eq!(
        records.len(),
        2,
        "the attempt-16 failure and the budget terminal each emit one record: {records:?}"
    );
    let failed = recorded_json_fields(&records[0]);
    assert_eq!(failed["policy_decision"], "executed");
    assert_eq!(failed["execution_status"], "failed");
    assert_eq!(failed["failure_code"], "executor_unavailable");
    assert!(failed["attempt_id"].is_string());
    let budget = recorded_json_fields(&records[1]);
    assert_eq!(budget["policy_decision"], "attempt_limit");
    assert_eq!(budget["failure_code"], "attempt_limit");
    assert!(
        budget["attempt_id"].is_null() && budget["approval_id"].is_null(),
        "the budget terminal allocates no new D-6/D-7"
    );
    assert_eq!(budget["descriptor_id"], "fixture.tool");
    assert_eq!(budget["profile_id"], "profile-default");
    assert_eq!(
        budget["action_digest"].as_str().map(str::len),
        Some(64),
        "the budget record correlates the exact action"
    );
    for serialized in &records {
        assert!(serialized.len() <= MAX_AUDIT_RECORD_BYTES);
    }
}

#[test]
fn record_stage_budget_exhaustion_also_emits_the_terminal_record() {
    // The same delivered failed/attempt_limit terminal reaches the model when
    // the retry's durable prepared record is rejected with the typed attempt
    // limit instead of the local allocation: the single exhaustion emission
    // point covers both branches (ADR-0003 TC-08/TC-14).
    let root = ToolExecutionRuntimeRoot::issue();
    let mut boundary = ToolExecutionAssembly::new(&root, snapshot(Effect::ReadData)).boundary(
        PreEffectFailingExecutor,
        AlwaysCurrentLease,
        BudgetLimitedCommitter::default(),
    );
    let mut audits = RecordingAudits::default();
    let outcome = boundary
        .execute_projected(
            &inputs(Effect::ReadData, u64::MAX),
            &scoped_trust(),
            &mut |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0),
            &mut || 1_000,
            &mut NoToolProjections,
            &mut audits,
        )
        .expect("the budget-exhausted retry reaches a terminal outcome");

    assert!(matches!(
        outcome,
        ToolExecutionOutcome::Failed {
            code: koduck_ai::application::ExecutionFailure::AttemptLimit,
            effect_state: EffectState::NotStarted,
        }
    ));
    let records = audits.serialized();
    assert_eq!(records.len(), 2, "found {records:?}");
    let failed = recorded_json_fields(&records[0]);
    assert_eq!(failed["policy_decision"], "executed");
    assert_eq!(failed["execution_status"], "failed");
    let budget = recorded_json_fields(&records[1]);
    assert_eq!(budget["policy_decision"], "attempt_limit");
    assert!(
        budget["attempt_id"].is_null() && budget["approval_id"].is_null(),
        "the budget terminal allocates no new D-6/D-7"
    );
}

#[test]
fn audit_observation_times_are_read_at_each_emission() {
    // A strictly stepping controlled clock separates every read, so an audit
    // observation time read once before approval resolution would freeze
    // every later terminal of the same pass at that earlier instant. The
    // D-6 resolution is observed only after its decision, and the D-7
    // execution terminal only after the resolution, so each record's
    // `at_millis` must be a read at its own emission (ADR-0003 TC-14).
    let mut tick = 1_000_u64;
    let decided_at = 1_500_u64;
    let root = ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, snapshot(Effect::ExternalWrite));
    let mut boundary = assembly.boundary(SucceedingExecutor, AlwaysCurrentLease, WinningCommitter);
    let mut audits = RecordingAudits::default();
    let outcome = boundary
        .execute_projected(
            &inputs(Effect::ExternalWrite, u64::MAX),
            &scoped_trust(),
            &mut move |_request: &ApprovalRequest| (ApprovalDecision::Accepted, decided_at),
            &mut move || {
                let now = tick;
                tick += 1_000;
                now
            },
            &mut NoToolProjections,
            &mut audits,
        )
        .expect("the accepted approval dispatches");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(audits.serialized().len(), 2);
    let resolution = recorded_json_fields(&audits.serialized()[0]);
    let terminal = recorded_json_fields(&audits.serialized()[1]);
    let resolution_at = resolution["at_millis"].as_u64().expect("at_millis is set");
    let terminal_at = terminal["at_millis"].as_u64().expect("at_millis is set");
    assert!(
        resolution_at >= decided_at,
        "the D-6 resolution is observed at or after its decision: {resolution_at} < {decided_at}"
    );
    assert!(
        terminal_at > resolution_at,
        "the D-7 terminal is observed after the D-6 resolution under a stepping clock: \
         {terminal_at} <= {resolution_at}"
    );
}

#[test]
fn interruption_close_emits_the_cancelled_terminal() {
    let root = ToolExecutionRuntimeRoot::issue();
    let sealed = koduck_ai::application::ToolAuthorizationService::new(snapshot(Effect::ReadData))
        .authorize_binding(
            ExactActionBinding::new(
                TenantId::new("tenant-a").expect("valid tenant"),
                ThreadId::new(),
                TurnId::new(),
                LeaseGeneration::initial(),
                ("profile-default", "v1"),
                AttemptId::new(),
                Action::new(
                    "fixture.tool",
                    "v1",
                    Effect::ReadData,
                    "fixture-target",
                    parse_action_parameters(r"{}").expect("valid parameters"),
                )
                .expect("valid action"),
            )
            .expect("valid binding"),
        )
        .expect("binding is policy-authorized");
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let (_authority, _attempt) = preparer
        .prepare(sealed.clone())
        .expect("the attempt prepares locally");
    let mut coordinator =
        ExecutionCoordinator::new(SucceedingExecutor, AlwaysCurrentLease, WinningCommitter);
    let mut audits = RecordingAudits::default();
    let outcome = root
        .runtime()
        .interrupter()
        .interrupt(
            &mut coordinator,
            &mut audits,
            &mut NoApprovals,
            sealed.tenant_id(),
            sealed.thread_id(),
            sealed.turn_id(),
            &mut || 1_000,
        )
        .expect("the interruption closes the prepared attempt");
    assert!(matches!(
        outcome,
        koduck_ai::application::InterruptionOutcome::Closed(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted
        })
    ));
    assert_eq!(audits.serialized().len(), 1);
    let fields = recorded_json_fields(&audits.serialized()[0]);
    assert_eq!(fields["policy_decision"], "executed");
    assert_eq!(fields["execution_status"], "cancelled");
    assert_eq!(
        fields["attempt_id"],
        sealed.attempt_id().as_uuid().to_string()
    );
}

/// Pending-approval double for the interruption leg: the read-data D-7
/// requires no D-6, so the canceller is never consulted.
struct NoApprovals;

impl koduck_ai::application::PendingApprovalCanceller for NoApprovals {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PendingApprovalCancellation, ExecutionPending> {
        unreachable!("a read-data attempt has no requested D-6")
    }
}
