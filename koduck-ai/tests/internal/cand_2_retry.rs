// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use std::collections::VecDeque;

use crate::test_support::process_local_durable_claims;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, ApprovalAuthorizer, ApprovalDecisionService, AttemptCommitError,
    AttemptCommitResult, AttemptCommitter, AttemptTerminalResolution, CancelAcknowledgement,
    CancelPermit, DispatchPermit, EffectState, ExecutionCoordinator, ExecutionFailure,
    ExecutionPending, ExecutionPreparationError, ExecutionResponse, ExecutionResponseBuilder,
    ExecutorError, IsolatedExecutor, LeaseCheck, LeaseValidator, ToolAuthorizationService,
    ToolCallError, ToolCallInputs, ToolExecutionAuthorityRoot, ToolExecutionDriver,
    ToolExecutionOutcome, ToolExecutionRuntime, ToolProjection, ToolProjectionError,
    ToolProjectionSink,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalRequest, AttemptId, ExactActionBinding, ExecutionError,
    ExecutionStatus,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

struct AlwaysCurrentLease;

impl LeaseValidator for AlwaysCurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Lease that plays a fixed sequence of decisions, so the preparer and
/// coordinator can share one lease type while fencing at different points.
struct SequencedLease {
    decisions: VecDeque<bool>,
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

/// Executor that plays a fixed script and records every dispatched D-7 identity.
struct ScriptedExecutor {
    responses: VecDeque<Result<ExecutionResponse, ExecutorError>>,
    seen: Vec<AttemptId>,
}

impl IsolatedExecutor for ScriptedExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.seen.push(binding.attempt_id());
        self.responses
            .pop_front()
            .expect("the scripted executor provides one response per dispatch")
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

struct WinningCommitter {
    calls: usize,
}

/// Captures the C-5 driver's durable views without granting them authority.
#[derive(Default)]
struct ProjectionRecorder {
    appended: Vec<ToolProjection>,
}

impl ToolProjectionSink for ProjectionRecorder {
    fn append(&mut self, projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        self.appended.push(projection.clone());
        Ok(())
    }

    fn publish(&mut self, _projection: &ToolProjection) {}
}

impl AttemptCommitter for WinningCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.calls += 1;
        Ok(AttemptCommitResult::Won)
    }
}

/// Committer that always reports a conflicting canonical terminal.
struct ConflictCommitter;

/// Committer whose dedicated post-dispatch fenced transition has durably won.
struct FencedTerminalCommitter;

impl AttemptCommitter for ConflictCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Err(AttemptCommitError::Conflict)
    }
}

impl AttemptCommitter for FencedTerminalCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        unreachable!("a post-dispatch fence uses the dedicated terminal transition")
    }

    fn commit_fenced_after_dispatch(
        &mut self,
        _binding: &ExactActionBinding,
        _effect_state: EffectState,
        _terminal_at_millis: u64,
    ) -> Result<AttemptTerminalResolution, koduck_ai::application::AttemptStoreError> {
        Ok(AttemptTerminalResolution::Won { version: 3 })
    }
}

struct FixturePolicyConfiguration {
    descriptor: CapabilityDescriptor,
    profile: PermissionProfile,
}

impl koduck_ai::application::ToolPolicyConfiguration for FixturePolicyConfiguration {
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
        _binding: &ExactActionBinding,
        _trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> bool {
        true
    }
}

fn unavailable(effect_state: EffectState) -> ExecutorError {
    ExecutorError::new(ExecutionFailure::ExecutorUnavailable, effect_state)
}

fn succeeded(output: &[u8]) -> ExecutionResponse {
    let mut response = ExecutionResponseBuilder::new(EffectState::Started);
    response
        .push_chunk(output)
        .expect("fixture response is within the output limit");
    response.finish().expect("fixture response remains bounded")
}

fn empty_executor() -> ScriptedExecutor {
    ScriptedExecutor {
        responses: VecDeque::new(),
        seen: Vec::new(),
    }
}

/// A clock that always returns the same timestamp (for tests that do not exercise
/// approval/delay ordering).
fn fixed_clock(timestamp: u64) -> impl FnMut() -> u64 {
    move || timestamp
}

fn action_for(effect: Effect) -> Action {
    Action::new(
        "fixture.tool",
        "v1",
        effect,
        "fixture-target",
        parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action")
}

fn config_for(effect: Effect) -> FixturePolicyConfiguration {
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        effect,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
        )
        .expect("valid schema"),
    )
    .expect("valid descriptor");
    let profile = PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.tool", "v1", effect, "fixture-target")
        .expect("valid profile entry")
        .build();
    FixturePolicyConfiguration {
        descriptor,
        profile,
    }
}

fn inputs(action: Action) -> ToolCallInputs {
    ToolCallInputs {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action,
        turn_deadline_millis: 600_000,
    }
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust")
}

fn new_runtime() -> ToolExecutionRuntime {
    ToolExecutionRuntime::new(&ToolExecutionAuthorityRoot::new())
}

fn driver(
    config: FixturePolicyConfiguration,
) -> ToolExecutionDriver<FixturePolicyConfiguration, FixtureApprovalAuthorizer> {
    ToolExecutionDriver::new(
        ToolAuthorizationService::new(config),
        ApprovalDecisionService::new(FixtureApprovalAuthorizer),
    )
}

fn authorize_clone(
    config: &FixturePolicyConfiguration,
    binding: ExactActionBinding,
) -> ExactActionBinding {
    ToolAuthorizationService::new(FixturePolicyConfiguration {
        descriptor: config.descriptor.clone(),
        profile: config.profile.clone(),
    })
    .authorize_binding(binding)
    .expect("fixture binding is policy-authorized")
}

fn binding_from(inputs: &ToolCallInputs) -> ExactActionBinding {
    ExactActionBinding::new(
        inputs.tenant_id.clone(),
        inputs.thread_id,
        inputs.turn_id,
        inputs.lease_generation,
        (inputs.profile_id.clone(), inputs.profile_version.clone()),
        AttemptId::new(),
        inputs.action.clone(),
    )
    .expect("valid binding")
}

process_local_durable_claims!(ConflictCommitter);
process_local_durable_claims!(FencedTerminalCommitter);

/// Winning close double whose durable transitions count every canonical
/// terminal close — a dispatched terminal or a prepared-only cancellation —
/// so the retry legs can assert exactly-once closing (ADR-0003 TC-12).
impl koduck_ai::application::DurableAttemptTransitions for WinningCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<
        koduck_ai::application::AttemptInsertResolution,
        koduck_ai::application::AttemptStoreError,
    > {
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted)
    }

    fn claim_running(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<
        koduck_ai::application::DispatchClaimResolution,
        koduck_ai::application::AttemptStoreError,
    > {
        Ok(koduck_ai::application::DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &koduck_ai::domain::execution::ExactActionBinding,
    ) -> Result<
        koduck_ai::application::PreparedCloseResolution,
        koduck_ai::application::AttemptStoreError,
    > {
        self.calls += 1;
        Ok(koduck_ai::application::PreparedCloseResolution::Won { version: 3 })
    }
}

#[test]
fn fenced_post_dispatch_failure_projects_the_committed_terminal() {
    // This fails if `dispatch` returns the reconciliation error before
    // emitting the canonical terminal that the dedicated fenced transition
    // already committed. The sink must retain a D-3 failed/version-3 view so
    // replay cannot leave the D-7 at running (ADR-0003 TC-06/TC-07).
    let inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Ok(succeeded(b"effect"))]),
            seen: Vec::new(),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, false]),
        },
        FencedTerminalCommitter,
    );
    let mut projections = ProjectionRecorder::default();

    let error = driver(config_for(Effect::ReadData))
        .execute_projected(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Cancelled, 0),
            &mut fixed_clock(1_000),
            &mut projections,
            &mut koduck_ai::application::NoToolAudits,
        )
        .expect_err("a fenced post-dispatch effect still requires reconciliation");

    assert!(matches!(
        error,
        ToolCallError::Reconciliation(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state: EffectState::Started,
        })
    ));
    assert!(matches!(
        projections.appended.as_slice(),
        [
            ToolProjection::ToolCall {
                status: ExecutionStatus::Running,
                version: 2,
                ..
            },
            ToolProjection::ToolResult {
                status: ExecutionStatus::Failed,
                code: Some(ExecutionFailure::OwnerFencedAfterDispatch),
                effect_state: EffectState::Started,
                output_bytes: 0,
                output_digest: None,
                version: 3,
                ..
            }
        ]
    ));
}

#[test]
fn retry_succeeds_after_proven_not_started() {
    let inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Ok(succeeded(b"ok")),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("retry reaches a terminal outcome");

    assert_eq!(
        outcome,
        ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: EffectState::Started,
        }
    );
    let seen = &coordinator.executor().seen;
    assert_eq!(seen.len(), 2, "the driver dispatched the retry");
    assert_ne!(seen[0], seen[1], "the retry allocated a fresh D-7 identity");
    assert_eq!(
        coordinator.committer().calls,
        2,
        "both the failed and retried attempts committed a terminal"
    );
}

#[test]
fn retry_does_not_retry_after_started_or_unknown() {
    for terminal_effect in [EffectState::Started, EffectState::Unknown] {
        let inputs = inputs(action_for(Effect::ReadData));
        let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
        let mut coordinator = ExecutionCoordinator::new(
            ScriptedExecutor {
                responses: VecDeque::from([Err(unavailable(terminal_effect))]),
                seen: Vec::new(),
            },
            AlwaysCurrentLease,
            WinningCommitter { calls: 0 },
        );
        let outcome = driver(config_for(Effect::ReadData))
            .execute(
                &mut preparer,
                &mut coordinator,
                &inputs,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, 1_000),
                &mut fixed_clock(1_000),
            )
            .expect("a started or unknown failure is terminal");
        assert!(
            matches!(outcome, ToolExecutionOutcome::Failed { effect_state, .. } if effect_state == terminal_effect),
            "a {terminal_effect:?} failure must not retry"
        );
        assert_eq!(
            coordinator.executor().seen.len(),
            1,
            "only the proven-pre-effect case retries"
        );
    }
}

#[test]
fn succeeded_or_cancelled_outcomes_do_not_retry() {
    // A first-pass success does not retry even though it is not a failure.
    let inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Ok(succeeded(b"ok"))]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("a success is terminal");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(coordinator.executor().seen.len(), 1);

    // A pre-dispatch cancellation (owner fenced) reports NotStarted but must not retry.
    let mut preparer = new_runtime().preparer(SequencedLease {
        decisions: VecDeque::from([true]),
    });
    let mut coordinator = ExecutionCoordinator::new(
        empty_executor(),
        SequencedLease {
            decisions: VecDeque::from([false, false, false]),
        },
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("a cancellation is terminal");
    assert!(
        matches!(
            outcome,
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted
            }
        ),
        "a cancellation must not retry even when it reports NotStarted"
    );
    assert!(
        coordinator.executor().seen.is_empty(),
        "a pre-dispatch cancellation never calls the executor"
    );
}

#[test]
fn retry_at_most_once() {
    let inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Err(unavailable(EffectState::NotStarted)),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("the second NotStarted is a terminal outcome");

    assert!(
        matches!(
            outcome,
            ToolExecutionOutcome::Failed {
                effect_state: EffectState::NotStarted,
                ..
            }
        ),
        "the driver returns the second NotStarted without a third attempt"
    );
    assert_eq!(
        coordinator.executor().seen.len(),
        2,
        "retry happens at most once"
    );
}

#[test]
fn retry_uses_a_fresh_d6_for_approval_required() {
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Ok(succeeded(b"ok")),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let mut d6_count = 0;
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| {
                d6_count += 1;
                (ApprovalDecision::Accepted, 1_000)
            },
            &mut fixed_clock(1_000),
        )
        .expect("retry reaches a terminal outcome");

    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(d6_count, 2, "each attempt created and resolved a fresh D-6");
    assert_eq!(coordinator.executor().seen.len(), 2);
}

#[test]
fn retry_does_not_retry_when_budget_exhausted() {
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let config = config_for(Effect::ExternalWrite);
    // Pre-consume 15 of the 16 attempt slots on the same Turn authority so the
    // retry cannot allocate a second D-7.
    for _ in 0..15 {
        let sealed = authorize_clone(&config, binding_from(&inputs));
        preparer
            .prepare(sealed)
            .expect("a pre-consumption slot is available");
    }
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Err(unavailable(EffectState::NotStarted))]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let mut projections = ProjectionRecorder::default();
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute_projected(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
            &mut projections,
            &mut koduck_ai::application::NoToolAudits,
        )
        .expect("the budget-exhausted retry returns a terminal outcome");

    assert!(
        matches!(
            outcome,
            ToolExecutionOutcome::Failed {
                code: ExecutionFailure::AttemptLimit,
                effect_state: EffectState::NotStarted,
            }
        ),
        "AC-9: a budget-exhausted retry is failed/attempt_limit"
    );
    assert_eq!(
        coordinator.executor().seen.len(),
        1,
        "no second D-7 is dispatched after the budget is exhausted"
    );
    assert_eq!(
        coordinator.committer().calls,
        1,
        "only the initial D-7 terminal was committed"
    );
    assert!(matches!(
        projections.appended.last(),
        Some(ToolProjection::Denied { code, .. }) if code == "attempt_limit"
    ));
}

#[test]
fn retry_does_not_retry_when_reconciliation_is_required() {
    let inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Err(unavailable(EffectState::NotStarted))]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        ConflictCommitter,
    );
    let error = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect_err("a conflicting terminal commit is not a retryable outcome");

    assert!(
        matches!(error, ToolCallError::Reconciliation(_)),
        "reconciliation owns the next transition instead of retrying"
    );
    assert_eq!(
        coordinator.executor().seen.len(),
        1,
        "the driver does not redispatch when no canonical terminal won"
    );
}

#[test]
fn declined_or_cancelled_approval_cancels_the_prepared_d7() {
    for decision in [ApprovalDecision::Declined, ApprovalDecision::Cancelled] {
        let inputs = inputs(action_for(Effect::ExternalWrite));
        let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
        let mut coordinator = ExecutionCoordinator::new(
            empty_executor(),
            AlwaysCurrentLease,
            WinningCommitter { calls: 0 },
        );
        let outcome = driver(config_for(Effect::ExternalWrite))
            .execute(
                &mut preparer,
                &mut coordinator,
                &inputs,
                &trust(),
                &mut |_| (decision, 1_000),
                &mut fixed_clock(1_000),
            )
            .expect("a non-accepted decision closes the D-7");

        assert!(
            matches!(
                outcome,
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted
                }
            ),
            "a {decision:?} approval must cancel the prepared D-7 without dispatch"
        );
        assert!(
            coordinator.executor().seen.is_empty(),
            "the executor is never called for a non-accepted approval"
        );
        assert_eq!(
            coordinator.committer().calls,
            1,
            "the cancelled D-7 terminal is committed once"
        );
    }
}

#[test]
fn late_decision_time_expires_the_d6_and_cancels_the_d7() {
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        empty_executor(),
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    // now=1_000 and the five-minute D-6 window expires at 301_000; a decision
    // returned at 301_001 must be honored as expired (not the call-start time).
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 301_001),
            &mut fixed_clock(1_000),
        )
        .expect("an expired D-6 cancels the prepared D-7");

    assert!(
        matches!(
            outcome,
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted
            }
        ),
        "a decision arriving after the D-6 expiry cancels the prepared D-7"
    );
    assert!(
        coordinator.executor().seen.is_empty(),
        "the executor is never called once the D-6 has expired"
    );
}

#[test]
fn owner_fenced_during_retry_does_not_deliver_a_result() {
    let inputs = inputs(action_for(Effect::ReadData));
    // The preparer's lease is current for the first prepare, then fences the retry.
    let mut preparer = new_runtime().preparer(SequencedLease {
        decisions: VecDeque::from([true, false]),
    });
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Err(unavailable(EffectState::NotStarted))]),
            seen: Vec::new(),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        WinningCommitter { calls: 0 },
    );
    let error = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect_err("a fenced retry must not deliver a stale committed result");

    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::OwnerFenced)
        ),
        "an owner fenced during retry preparation must not return the committed terminal"
    );
    assert_eq!(
        coordinator.executor().seen.len(),
        1,
        "only the first attempt dispatched before the retry fence"
    );
}

#[test]
fn retry_reads_a_fresh_clock_for_each_d6_and_dispatch() {
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Ok(succeeded(b"ok")),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    // Each D-6 receives a later creation time, each preparation record reads
    // the clock between the creation and the dispatch start, each audit
    // terminal — the D-6 resolution and the D-7 terminal — reads the clock at
    // its own emission, and the dispatch and response reads remain inside
    // their respective 30-second action budgets.
    let mut clock_reads = VecDeque::from([
        1_000, 2_000, 3_000, 101_000, 301_000, 301_001, 301_002, 401_000, 402_000, 403_000,
        501_000, 600_000, 600_001, 600_002,
    ]);
    let mut clock = || {
        clock_reads.pop_front().expect(
            "fixture supplies every creation, preparation, dispatch, and response clock read",
        )
    };
    let mut observed_expires_at = Vec::new();
    let mut decision = |request: &ApprovalRequest| {
        observed_expires_at.push(request.expires_at_millis());
        (ApprovalDecision::Accepted, request.expires_at_millis() - 1)
    };
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut decision,
            &mut clock,
        )
        .expect("retry reaches a terminal outcome");

    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(
        observed_expires_at.len(),
        2,
        "each attempt created a fresh D-6"
    );
    assert!(
        observed_expires_at[1] > observed_expires_at[0],
        "the retry D-6 window starts from a later clock read, not the original call time"
    );
}

#[test]
fn dispatch_start_time_is_not_earlier_than_the_approval_decision() {
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Ok(succeeded(b"ok"))]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    // The decision arrives at 250_000 while the clock reads only 1_000; the D-7
    // start time must still never precede the verified decision time.
    let decided_at = 250_000u64;
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, decided_at),
            &mut fixed_clock(1_000),
        )
        .expect("dispatch reaches a terminal outcome");

    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert!(
        coordinator.last_started_at_millis() >= decided_at,
        "the D-7 start time must not precede the approval decision"
    );
}

/// AC-9: automatic retry occurs only once after a proven not-started executor
/// effect, gets fresh authority, and consumes another attempt slot. The legs
/// consolidate the declared boundary table: a read-only not-started failure
/// with budget retries on a fresh D-7 that consumes a second slot; a
/// privileged retry resolves a second distinct D-6; started/unknown failures
/// never retry; and an initial action that consumes slot 16 allocates and
/// dispatches no retry, terminating `failed/attempt_limit`.
// One cohesive AC-9 harness whose four budget/approval/dispatch legs share the
// scripted fixtures; the ADR acceptance command pins this exact test name.
#[allow(clippy::too_many_lines)]
#[test]
fn pre_effect_retry_requires_fresh_attempt_and_policy() {
    // Leg 1 — read-only not-started failure with budget available: exactly two
    // distinct D-7 identities, and the pair consumes two of the 16 slots.
    let readonly_inputs = inputs(action_for(Effect::ReadData));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let config = config_for(Effect::ReadData);
    for _ in 0..14 {
        let sealed = authorize_clone(&config, binding_from(&readonly_inputs));
        preparer
            .prepare(sealed)
            .expect("a pre-consumption slot is available");
    }
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Ok(succeeded(b"ok")),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ReadData))
        .execute(
            &mut preparer,
            &mut coordinator,
            &readonly_inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("the budgeted retry reaches a terminal outcome");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    let seen = &coordinator.executor().seen;
    assert_eq!(seen.len(), 2, "the retry dispatched a second attempt");
    assert_ne!(seen[0], seen[1], "the retry used a fresh D-7 identity");
    assert_eq!(
        coordinator.committer().calls,
        2,
        "both attempts committed a terminal"
    );
    // The two driver attempts consumed slots 15 and 16: the next allocation on
    // this Turn authority is rejected with the exact attempt-limit code.
    let sealed = authorize_clone(&config, binding_from(&readonly_inputs));
    assert!(
        matches!(
            preparer.prepare(sealed),
            Err(ExecutionPreparationError::Rejected(
                ExecutionError::AttemptLimit
            ))
        ),
        "the initial action and its retry consumed two attempt slots"
    );

    // Leg 2 — privileged not-started failure: the retry resolves a second,
    // distinct accepted D-6 before its fresh D-7 dispatches.
    let privileged_inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([
                Err(unavailable(EffectState::NotStarted)),
                Ok(succeeded(b"ok")),
            ]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let mut observed_d6 = Vec::new();
    let mut decision = |request: &ApprovalRequest| {
        observed_d6.push(request.approval_id());
        (ApprovalDecision::Accepted, 1_000)
    };
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &privileged_inputs,
            &trust(),
            &mut decision,
            &mut fixed_clock(1_000),
        )
        .expect("the privileged retry reaches a terminal outcome");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    assert_eq!(observed_d6.len(), 2, "each attempt resolved its own D-6");
    assert_ne!(
        observed_d6[0], observed_d6[1],
        "the retry required a fresh accepted D-6"
    );
    assert_eq!(coordinator.executor().seen.len(), 2);

    // Leg 3 — started or unknown failures never retry: one D-7, one dispatch.
    for terminal_effect in [EffectState::Started, EffectState::Unknown] {
        let inputs = inputs(action_for(Effect::ReadData));
        let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
        let mut coordinator = ExecutionCoordinator::new(
            ScriptedExecutor {
                responses: VecDeque::from([Err(unavailable(terminal_effect))]),
                seen: Vec::new(),
            },
            AlwaysCurrentLease,
            WinningCommitter { calls: 0 },
        );
        let outcome = driver(config_for(Effect::ReadData))
            .execute(
                &mut preparer,
                &mut coordinator,
                &inputs,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, 1_000),
                &mut fixed_clock(1_000),
            )
            .expect("a started or unknown failure is terminal");
        assert!(
            matches!(outcome, ToolExecutionOutcome::Failed { effect_state, .. } if effect_state == terminal_effect),
            "a {terminal_effect:?} failure must not retry"
        );
        assert_eq!(
            coordinator.executor().seen.len(),
            1,
            "only the proven-pre-effect failure retries"
        );
    }

    // Leg 4 — the initial action consumes slot 16: no retry record or dispatch
    // is created and the action terminates failed/attempt_limit.
    let inputs = inputs(action_for(Effect::ExternalWrite));
    let mut preparer = new_runtime().preparer(AlwaysCurrentLease);
    let config = config_for(Effect::ExternalWrite);
    for _ in 0..15 {
        let sealed = authorize_clone(&config, binding_from(&inputs));
        preparer
            .prepare(sealed)
            .expect("a pre-consumption slot is available");
    }
    let mut coordinator = ExecutionCoordinator::new(
        ScriptedExecutor {
            responses: VecDeque::from([Err(unavailable(EffectState::NotStarted))]),
            seen: Vec::new(),
        },
        AlwaysCurrentLease,
        WinningCommitter { calls: 0 },
    );
    let outcome = driver(config_for(Effect::ExternalWrite))
        .execute(
            &mut preparer,
            &mut coordinator,
            &inputs,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, 1_000),
            &mut fixed_clock(1_000),
        )
        .expect("the slot-16 action returns a terminal outcome");
    assert!(
        matches!(
            outcome,
            ToolExecutionOutcome::Failed {
                code: ExecutionFailure::AttemptLimit,
                effect_state: EffectState::NotStarted,
            }
        ),
        "after the initial action consumes slot 16 the action is failed/attempt_limit"
    );
    assert_eq!(
        coordinator.executor().seen.len(),
        1,
        "no retry dispatch follows the exhausted budget"
    );
    assert_eq!(
        coordinator.committer().calls,
        1,
        "no retry terminal was committed"
    );
}
