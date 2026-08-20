// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use std::collections::VecDeque;

use crate::test_support::process_local_durable_claims;
use koduck_ai::adapters::execution::DisabledExecutor;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, ApprovalAuthorizer, ApprovalDecisionService, AttemptCommitError,
    AttemptCommitResult, AttemptCommitter, AttemptInsertResolution, AttemptTerminalResolution,
    CancelAcknowledgement, CancelPermit, CanonicalAttemptTerminal, CanonicalTerminalError,
    DenialCode, DispatchClaimResolution, DispatchPermit, EffectState, ExecutionCoordinator,
    ExecutionFailure, ExecutionPending, ExecutionPreparationError, ExecutionResponse,
    ExecutionResponseBuilder, ExecutorError, IsolatedExecutor, LeaseCheck, LeaseValidator,
    PolicyDecision, PreparedCloseResolution, ToolAuthorizationService, ToolExecutionAuthorityRoot,
    ToolExecutionOutcome, ToolExecutionRuntime, ToolPolicy, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ApprovalStatus, AttemptId,
    ExactActionBinding, ExecutionAttempt, ExecutionStatus, TurnExecutionAuthority,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

pub(super) struct RecordingExecutor {
    calls: usize,
    response: Result<ExecutionResponse, ExecutorError>,
}

impl IsolatedExecutor for RecordingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.calls += 1;
        self.response.clone()
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}

struct RecordingCommitter {
    calls: usize,
    result: Result<AttemptCommitResult, AttemptCommitError>,
}

impl AttemptCommitter for RecordingCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.calls += 1;
        self.result.clone()
    }
}

pub(super) struct SequencedLease {
    decisions: VecDeque<bool>,
}

fn new_runtime() -> ToolExecutionRuntime {
    ToolExecutionRuntime::new(&ToolExecutionAuthorityRoot::new())
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

fn accepted() -> (ExactActionBinding, ApprovalRequest) {
    let action = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action");
    accepted_for(action)
}

fn accepted_for(action: Action) -> (ExactActionBinding, ApprovalRequest) {
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        action,
    )
    .expect("valid binding");
    let binding = authorize(binding).expect("fixture action is policy-authorized");
    let mut approval = ApprovalRequest::new(binding.clone(), 0, 600_000).expect("valid approval");
    resolve(&mut approval, ApprovalDecision::Accepted, 1).expect("accepted");
    (binding, approval)
}

fn prepared(binding: ExactActionBinding) -> (TurnExecutionAuthority, ExecutionAttempt) {
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    preparer
        .prepare(binding)
        .expect("current owner has an attempt slot")
}

fn authorize(binding: ExactActionBinding) -> Result<ExactActionBinding, DenialCode> {
    authorize_for_profile(binding, "profile-default")
}

fn authorize_for_profile(
    binding: ExactActionBinding,
    profile_id: &str,
) -> Result<ExactActionBinding, DenialCode> {
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

struct FixtureApprovalAuthorizer;

impl ApprovalAuthorizer for FixtureApprovalAuthorizer {
    fn can_resolve_tool_approval(
        &mut self,
        binding: &ExactActionBinding,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> bool {
        binding.tenant_id() == &trust.tenant_id && binding.thread_id() == thread_id
    }
}

fn resolve(
    approval: &mut ApprovalRequest,
    decision: ApprovalDecision,
    decided_at_millis: u64,
) -> Result<u64, koduck_ai::domain::execution::ApprovalError> {
    let thread_id = approval.thread_id();
    let trust = TrustContext::new(approval.tenant_id().clone(), "approver-a")
        .expect("valid authenticated principal");
    ApprovalDecisionService::new(FixtureApprovalAuthorizer).resolve(
        approval,
        &trust,
        thread_id,
        decision,
        decided_at_millis,
    )
}

#[path = "cand_2_authority_reclamation.rs"]
mod authority_reclamation;
#[path = "cand_2_policy_isolation.rs"]
mod policy_isolation;
#[path = "cand_2_execution_durable_gates.rs"]
mod durable_gates;
#[path = "cand_2_execution_transport.rs"]
mod transport;

pub(super) fn response(effect_state: EffectState, output: &[u8]) -> ExecutionResponse {
    let mut response = ExecutionResponseBuilder::new(effect_state);
    response
        .push_chunk(output)
        .expect("fixture response is within the output limit");
    response.finish().expect("fixture response remains bounded")
}

fn committer(result: Result<(), AttemptCommitError>) -> RecordingCommitter {
    RecordingCommitter {
        calls: 0,
        result: result.map(|()| AttemptCommitResult::Won),
    }
}

process_local_durable_claims!(RecordingCommitter);

#[test]
fn fencing_before_dispatch_makes_no_executor_call() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([false]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().calls, 0);
}

#[test]
fn fenced_owner_cannot_prepare_or_consume_attempt_budget() {
    let (binding, _approval) = accepted();
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([false]),
    });

    assert!(matches!(
        preparer.prepare(binding),
        Err(ExecutionPreparationError::OwnerFenced)
    ));
}

#[test]
fn fenced_binding_cannot_seed_the_turn_profile() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let binding = |profile: &str| {
        let binding = ExactActionBinding::new(
            TenantId::new("tenant-a").expect("valid tenant"),
            thread_id,
            turn_id,
            LeaseGeneration::initial(),
            (profile, "v1"),
            AttemptId::new(),
            Action::new(
                "fixture.tool",
                "v1",
                Effect::ExternalWrite,
                "fixture-target",
                parse_action_parameters("{}").expect("valid parameters"),
            )
            .expect("valid action"),
        )
        .expect("valid binding");
        authorize_for_profile(binding, profile).expect("fixture profile authorizes binding")
    };
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([false, true]),
    });

    assert!(matches!(
        preparer.prepare(binding("profile-stale")),
        Err(ExecutionPreparationError::OwnerFenced)
    ));
    let (authority, _attempt) = preparer
        .prepare(binding("profile-current"))
        .expect("current binding establishes the Turn profile");
    assert_eq!(authority.used(), 1);
}

#[test]
fn denied_action_cannot_reach_execution_preparation() {
    let action = Action::new(
        "fixture.tool",
        "v1",
        Effect::ExternalWrite,
        "fixture-target",
        parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action");
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        action.clone(),
    )
    .expect("valid binding");
    let profile = PermissionProfile::empty("profile-default", "v1").expect("valid profile");
    assert_eq!(
        ToolPolicy.evaluate(None, &action, &profile),
        PolicyDecision::Denied(DenialCode::DescriptorMissing)
    );
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });

    assert!(
        preparer.prepare(binding).is_err(),
        "a denied binding must not allocate a D-7 slot"
    );
}

#[test]
fn policy_authorized_read_dispatches_without_creating_d6() {
    let action = Action::new(
        "fixture.read",
        "v1",
        Effect::ReadData,
        "fixture-target",
        parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action");
    let descriptor = CapabilityDescriptor::new(
        "fixture.read",
        "v1",
        Effect::ReadData,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
        )
        .expect("valid schema"),
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
        .expect("valid profile entry")
        .build();
    let binding = ExactActionBinding::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        ThreadId::new(),
        TurnId::new(),
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        action,
    )
    .expect("valid binding");
    let binding = ToolAuthorizationService::new(FixturePolicyConfiguration {
        descriptor,
        profile,
    })
    .authorize_binding(binding)
    .expect("read is authorized without approval");
    assert!(matches!(
        ApprovalRequest::new(binding.clone(), 0, 600_000),
        Err(ApprovalError::ApprovalNotRequired)
    ));
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    let (mut authority, mut attempt) = preparer.prepare(binding).expect("read prepares");
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"read-result")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        committer(Ok(())),
    );

    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 1, &mut || 1),
        Ok(ToolExecutionOutcome::Succeeded {
            output: b"read-result".to_vec(),
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().calls, 1);
}

#[test]
fn separate_preparers_share_one_turn_running_slot() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let binding = |attempt_id| {
        ExactActionBinding::new(
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
                parse_action_parameters("{}").expect("valid parameters"),
            )
            .expect("valid action"),
        )
        .expect("valid binding")
    };
    let first_binding = binding(koduck_ai::domain::execution::AttemptId::new());
    let second_binding = binding(koduck_ai::domain::execution::AttemptId::new());
    let first_binding = authorize(first_binding).expect("first binding is authorized");
    let second_binding = authorize(second_binding).expect("second binding is authorized");
    let mut first_approval =
        koduck_ai::domain::execution::ApprovalRequest::new(first_binding.clone(), 0, 600_000)
            .expect("valid approval");
    resolve(&mut first_approval, ApprovalDecision::Accepted, 1).expect("accepted");
    let mut second_approval =
        koduck_ai::domain::execution::ApprovalRequest::new(second_binding.clone(), 0, 600_000)
            .expect("valid approval");
    resolve(&mut second_approval, ApprovalDecision::Accepted, 1).expect("accepted");
    let runtime = new_runtime();
    let mut first_preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    let mut second_preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    let (mut first_authority, mut first_attempt) = first_preparer
        .prepare(first_binding)
        .expect("first attempt prepares");
    let (mut second_authority, mut second_attempt) = second_preparer
        .prepare(second_binding)
        .expect("second attempt prepares");

    first_authority
        .claim_dispatch(&mut first_attempt, Some(&first_approval), 2)
        .expect("first attempt starts");
    assert!(
        second_authority
            .claim_dispatch(&mut second_attempt, Some(&second_approval), 3)
            .is_err(),
        "a second preparer must observe the existing running attempt"
    );
}

#[test]
fn runtime_does_not_forget_a_live_turn_after_all_handles_temporarily_drop() {
    let (binding, _approval) = accepted();
    let runtime = new_runtime();
    {
        let mut preparer = runtime.preparer(SequencedLease {
            decisions: VecDeque::from([true]),
        });
        let (authority, attempt) = preparer
            .prepare(binding.clone())
            .expect("first allocation succeeds");
        drop(attempt);
        drop(authority);
        drop(preparer);
    }
    let mut reconstructed = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });

    assert!(matches!(
        reconstructed.prepare(binding),
        Err(ExecutionPreparationError::Rejected(
            koduck_ai::domain::execution::ExecutionError::AttemptAlreadyAllocated
        ))
    ));
}

#[test]
fn separately_constructed_runtime_handles_cannot_reset_turn_authority() {
    let (binding, _approval) = accepted();
    let root = ToolExecutionAuthorityRoot::new();
    let first_runtime = ToolExecutionRuntime::new(&root);
    let second_runtime = ToolExecutionRuntime::new(&root);
    let mut first = first_runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    first
        .prepare(binding.clone())
        .expect("first runtime handle allocates the attempt");
    let mut second = second_runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });

    assert!(matches!(
        second.prepare(binding),
        Err(ExecutionPreparationError::Rejected(
            koduck_ai::domain::execution::ExecutionError::AttemptAlreadyAllocated
        ))
    ));
}

#[test]
fn coordinator_preserves_the_concurrent_attempt_code() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let binding = |attempt_id| {
        ExactActionBinding::new(
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
                parse_action_parameters("{}").expect("valid parameters"),
            )
            .expect("valid action"),
        )
        .expect("valid binding")
    };
    let first_binding = binding(koduck_ai::domain::execution::AttemptId::new());
    let second_binding = binding(koduck_ai::domain::execution::AttemptId::new());
    let first_binding = authorize(first_binding).expect("first binding is authorized");
    let second_binding = authorize(second_binding).expect("second binding is authorized");
    let mut first_approval =
        koduck_ai::domain::execution::ApprovalRequest::new(first_binding.clone(), 0, 600_000)
            .expect("valid approval");
    resolve(&mut first_approval, ApprovalDecision::Accepted, 1).expect("accepted");
    let mut second_approval =
        koduck_ai::domain::execution::ApprovalRequest::new(second_binding.clone(), 0, 600_000)
            .expect("valid approval");
    resolve(&mut second_approval, ApprovalDecision::Accepted, 1).expect("accepted");
    let runtime = new_runtime();
    let mut preparer = runtime.preparer(SequencedLease {
        decisions: VecDeque::from([true, true]),
    });
    let (mut first_authority, mut first_attempt) = preparer
        .prepare(first_binding)
        .expect("first attempt prepares");
    let (mut second_authority, mut second_attempt) = preparer
        .prepare(second_binding)
        .expect("second attempt prepares");
    first_authority
        .claim_dispatch(&mut first_attempt, Some(&first_approval), 2)
        .expect("first attempt starts");
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::Started, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        committer(Ok(())),
    );

    assert_eq!(
        coordinator.execute(
            &mut second_authority,
            Some(&second_approval),
            &mut second_attempt,
            3,
            &mut || 3,
        ),
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::ConcurrentAttempt,
        })
    );
    assert_eq!(coordinator.executor().calls, 0);
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(second_attempt.status(), ExecutionStatus::Prepared);
}

#[test]
fn disabled_executor_fails_closed_without_fallback() {
    let (binding, approval) = accepted();
    let executor = RecordingExecutor {
        calls: 0,
        response: Err(ExecutorError::new(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::NotStarted,
        )),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));
    let (mut authority, mut attempt) = prepared(binding);

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Failed {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().calls, 1);
}

#[test]
fn production_disabled_executor_has_no_effect_path() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(DisabledExecutor, lease, committer(Ok(())));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Failed {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::NotStarted,
        })
    );
}

#[test]
fn accepted_attempt_cannot_dispatch_twice() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true, true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    let _first = coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2);
    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 3, &mut || 3),
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::ApprovalAlreadyConsumed,
        })
    );

    assert_eq!(
        coordinator.executor().calls,
        1,
        "one canonical D-7 may dispatch only once"
    );
}

#[test]
fn executor_error_without_effect_evidence_is_unknown() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Err(ExecutorError::new(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::Unknown,
        )),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Failed {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::Unknown,
        })
    );
}

#[test]
fn fenced_executor_error_never_commits_a_terminal() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Err(ExecutorError::new(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::Unknown,
        )),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, false]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Running);
}

#[test]
fn successful_outcome_preserves_executor_effect_state() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(ToolExecutionOutcome::Succeeded {
            output: b"result".to_vec(),
            effect_state: EffectState::Started,
        })
    );
}

#[test]
fn stale_replay_cannot_rewrite_a_terminal_attempt() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true, false]),
    };
    let mut coordinator = ExecutionCoordinator::new(executor, lease, committer(Ok(())));

    let first = coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2);
    let terminal_status = attempt.status();
    let replay = coordinator.execute(&mut authority, Some(&approval), &mut attempt, 3, &mut || 3);

    assert!(matches!(first, Ok(ToolExecutionOutcome::Succeeded { .. })));
    assert_eq!(attempt.status(), terminal_status);
    assert_eq!(
        replay,
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::ApprovalAlreadyConsumed,
        })
    );
}

#[test]
fn durable_commit_failure_never_reports_success() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator = ExecutionCoordinator::new(
        executor,
        lease,
        committer(Err(AttemptCommitError::Unavailable)),
    );

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::Started,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Running);
}

#[test]
fn lost_idempotent_commit_returns_the_existing_canonical_terminal() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let existing = ToolExecutionOutcome::Failed {
        code: ExecutionFailure::ExecutorUnavailable,
        effect_state: EffectState::Unknown,
    };
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::Started, b"losing-output")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        RecordingCommitter {
            calls: 0,
            result: Ok(AttemptCommitResult::Existing(Box::new(
                CanonicalAttemptTerminal::from_persistence(
                    attempt.binding().clone(),
                    3,
                    existing.clone(),
                )
                .expect("valid canonical terminal"),
            ))),
        },
    );

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Ok(existing)
    );
    assert_eq!(attempt.status(), ExecutionStatus::Failed);
}

#[test]
fn existing_terminal_that_cannot_update_the_local_mirror_requires_reconciliation() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let existing = ToolExecutionOutcome::Succeeded {
        output: b"canonical-output".to_vec(),
        effect_state: EffectState::Started,
    };
    let canonical =
        CanonicalAttemptTerminal::from_persistence(attempt.binding().clone(), 3, existing)
            .expect("bounded canonical terminal");
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([false]),
        },
        RecordingCommitter {
            calls: 0,
            result: Ok(AttemptCommitResult::Existing(Box::new(canonical))),
        },
    );

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Started,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Prepared);
    assert_eq!(coordinator.executor().calls, 0);
    assert!(
        authority.live_attempts().is_empty(),
        "a known canonical terminal must stay unavailable until reconciliation mirrors it"
    );
}

#[test]
fn existing_terminal_for_another_attempt_requires_reconciliation() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let (other_binding, _other_approval) = accepted();
    let existing = CanonicalAttemptTerminal::from_persistence(
        other_binding,
        2,
        ToolExecutionOutcome::Failed {
            code: ExecutionFailure::ExecutorUnavailable,
            effect_state: EffectState::Unknown,
        },
    )
    .expect("bounded terminal with a different identity");
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::Started, b"losing-output")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        RecordingCommitter {
            calls: 0,
            result: Ok(AttemptCommitResult::Existing(Box::new(existing))),
        },
    );

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Started,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Running);
}

#[test]
fn oversized_existing_terminal_never_reaches_the_model() {
    let (binding, approval) = accepted();
    let (_authority, attempt) = prepared(binding);
    let existing = ToolExecutionOutcome::Succeeded {
        output: vec![0; 1_048_577],
        effect_state: EffectState::Started,
    };
    assert_eq!(
        CanonicalAttemptTerminal::from_persistence(attempt.binding().clone(), 2, existing),
        Err(CanonicalTerminalError::OutputTooLarge)
    );
    assert_eq!(attempt.status(), ExecutionStatus::Prepared);
    assert_eq!(approval.status(), ApprovalStatus::Accepted);
}

#[test]
fn conflicting_terminal_commit_requires_reconciliation() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let mut coordinator = ExecutionCoordinator::new(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::Started, b"losing-output")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        committer(Err(AttemptCommitError::Conflict)),
    );

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Started,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Running);
    assert!(
        authority.live_attempts().is_empty(),
        "a conflicting canonical terminal must stay unavailable until reconciliation"
    );
}

#[test]
fn executor_output_is_bounded_while_streaming() {
    let mut response = ExecutionResponseBuilder::new(EffectState::Started);
    assert_eq!(response.push_chunk(&vec![0; 1_048_576]), Ok(()));
    assert_eq!(
        response.push_chunk(&[0]),
        Err(ExecutorError::new(
            ExecutionFailure::OutputLimitExceeded,
            EffectState::Started,
        ))
    );
    assert_eq!(
        response.finish(),
        Err(ExecutorError::new(
            ExecutionFailure::OutputLimitExceeded,
            EffectState::Started,
        ))
    );
}

#[test]
fn fenced_commit_before_dispatch_has_a_pre_dispatch_reason() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::Started, b"unused")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([false]),
    };
    let mut coordinator =
        ExecutionCoordinator::new(executor, lease, committer(Err(AttemptCommitError::Fenced)));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedBeforeDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().calls, 0);
    assert_eq!(attempt.status(), ExecutionStatus::Prepared);
}

#[test]
fn fenced_terminal_commit_remains_running_for_reconciliation() {
    let (binding, approval) = accepted();
    let (mut authority, mut attempt) = prepared(binding);
    let executor = RecordingExecutor {
        calls: 0,
        response: Ok(response(EffectState::NotStarted, b"result")),
    };
    let lease = SequencedLease {
        decisions: VecDeque::from([true, true, true]),
    };
    let mut coordinator =
        ExecutionCoordinator::new(executor, lease, committer(Err(AttemptCommitError::Fenced)));

    assert_eq!(
        coordinator.execute(&mut authority, Some(&approval), &mut attempt, 2, &mut || 2),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(attempt.status(), ExecutionStatus::Running);
}
