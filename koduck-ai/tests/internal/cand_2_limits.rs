// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! AC-10 exact policy and execution limit table for the public C-5 boundary.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::test_support::process_local_durable_claims;
use koduck_ai::adapters::tool::{ToolAdapterError, parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, AttemptCommitError, AttemptCommitResult, AttemptCommitter,
    CancelAcknowledgement, CancelPermit, DispatchPermit, EffectState, ExecutionFailure,
    ExecutionPending, ExecutionPreparationError, ExecutionResponse, ExecutionResponseBuilder,
    ExecutorError, IsolatedExecutor, LeaseCheck, LeaseValidator, MAX_EXECUTOR_OUTPUT_BYTES,
    TOOL_APPROVAL_SCOPE, ToolCallError, ToolCallInputs, ToolConfigurationSnapshot,
    ToolExecutionAssembly, ToolExecutionBoundary, ToolExecutionOutcome, ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, AttemptId, ExactActionBinding, ExecutionError,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, MAX_ACTION_INPUT_BYTES,
    PermissionProfile,
};
use koduck_ai::domain::{
    ApprovalScopes, LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId,
};
use koduck_ai::runtime::RuntimeState;

#[path = "cand_2_limits_budget.rs"]
mod budget;

/// Distributes one authority-root handle through the production runtime-state
/// access path.
fn production_root() -> ToolExecutionRuntimeRoot {
    RuntimeState::assemble().tool_execution_root()
}

const T0: u64 = 1_000;
const FIVE_MINUTES: u64 = 300_000;
const TWO_MINUTES: u64 = 120_000;
const PADDED_SCHEMA: &str = r#"{"type":"object","properties":{"pad":{"type":"string"}},"required":["pad"],"additionalProperties":false}"#;

#[derive(Clone, Copy)]
struct AlwaysCurrentLease;

impl LeaseValidator for AlwaysCurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Winning committer that shares its call counter with the test driver.
struct CountingCommitter {
    calls: Arc<Mutex<usize>>,
}

impl AttemptCommitter for CountingCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        *self.calls.lock().expect("committer counter is healthy") += 1;
        Ok(AttemptCommitResult::Won)
    }
}

/// Committer whose canonical terminal write always conflicts, retaining the
/// running D-7 reservation so a second attempt is rejected as concurrent.
struct AlwaysConflictCommitter;

impl AttemptCommitter for AlwaysConflictCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Err(AttemptCommitError::Conflict)
    }
}

/// One scripted executor response for the counting executor.
#[derive(Clone)]
enum Script {
    /// A bounded successful output with the reported effect state.
    Ok(&'static [u8], EffectState),
    /// A byte vector of the given length with the reported effect state.
    Sized(usize, EffectState),
    /// A typed executor failure with the reported effect state.
    Fail(ExecutionFailure, EffectState),
}

/// Executor that records every dispatched D-7 identity and plays a script.
struct CountingExecutor {
    script: VecDeque<Script>,
    dispatches: Arc<Mutex<Vec<AttemptId>>>,
}

impl IsolatedExecutor for CountingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.dispatches
            .lock()
            .expect("executor log is healthy")
            .push(binding.attempt_id());
        match self
            .script
            .pop_front()
            .expect("script covers every dispatch")
        {
            Script::Ok(output, effect_state) => {
                let mut response = ExecutionResponseBuilder::new(effect_state);
                response
                    .push_chunk(output)
                    .expect("scripted output is within the limit");
                response.finish()
            }
            Script::Sized(length, effect_state) => {
                let chunk = vec![b'a'; length];
                let mut response = ExecutionResponseBuilder::new(effect_state);
                match response.push_chunk(&chunk) {
                    Ok(()) => response.finish(),
                    Err(error) => Err(error),
                }
            }
            Script::Fail(code, effect_state) => Err(ExecutorError::new(code, effect_state)),
        }
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

fn descriptor(effect: Effect, schema: &str) -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        effect,
        DescriptorState::Active,
        parse_input_schema(schema).expect("valid fixture schema"),
    )
    .expect("valid fixture descriptor")
}

fn profile(effect: Effect) -> PermissionProfile {
    PermissionProfile::builder("profile-default", "v1")
        .expect("valid profile")
        .allow("fixture.tool", "v1", effect, "fixture-target")
        .expect("valid profile entry")
        .build()
}

fn config(effect: Effect) -> ToolConfigurationSnapshot {
    config_with_schema(
        effect,
        r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
    )
}

fn config_with_schema(effect: Effect, schema: &str) -> ToolConfigurationSnapshot {
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(descriptor(effect, schema))
        .expect("descriptor registration is unique");
    snapshot
        .register_profile(profile(effect))
        .expect("profile registration is unique");
    snapshot
}

fn action(effect: Effect, parameters: &str) -> Action {
    Action::new(
        "fixture.tool",
        "v1",
        effect,
        "fixture-target",
        parse_action_parameters(parameters).expect("valid parameters"),
    )
    .expect("valid fixture action")
}

fn inputs(action: Action, turn_deadline_millis: u64) -> ToolCallInputs {
    ToolCallInputs {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action,
        turn_deadline_millis,
    }
}

fn approver() -> TrustContext {
    trust().with_approval_scopes(ApprovalScopes::from_validated([TOOL_APPROVAL_SCOPE]))
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid trust context")
}

fn executor_for(script: Vec<Script>, dispatches: &Arc<Mutex<Vec<AttemptId>>>) -> CountingExecutor {
    CountingExecutor {
        script: VecDeque::from(script),
        dispatches: Arc::clone(dispatches),
    }
}

/// Creates one winning-commit boundary derived from `assembly`'s shared root.
fn boundary_from(
    assembly: &ToolExecutionAssembly,
    script: Vec<Script>,
    dispatches: &Arc<Mutex<Vec<AttemptId>>>,
) -> ToolExecutionBoundary<CountingExecutor, CountingCommitter> {
    assembly.boundary(
        executor_for(script, dispatches),
        AlwaysCurrentLease,
        CountingCommitter {
            calls: Arc::new(Mutex::new(0)),
        },
    )
}

fn boundary(
    configuration: ToolConfigurationSnapshot,
    script: Vec<Script>,
) -> (
    ToolExecutionBoundary<CountingExecutor, CountingCommitter>,
    Arc<Mutex<Vec<AttemptId>>>,
) {
    let dispatches = Arc::new(Mutex::new(Vec::new()));
    let boundary = boundary_from(
        &ToolExecutionAssembly::new(&production_root(), configuration),
        script,
        &dispatches,
    );
    (boundary, dispatches)
}

/// Creates one conflict-commit boundary derived from `assembly`'s shared root.
fn conflicting_boundary_from(
    assembly: &ToolExecutionAssembly,
    script: Vec<Script>,
    dispatches: &Arc<Mutex<Vec<AttemptId>>>,
) -> ToolExecutionBoundary<CountingExecutor, AlwaysConflictCommitter> {
    assembly.boundary(
        executor_for(script, dispatches),
        AlwaysCurrentLease,
        AlwaysConflictCommitter,
    )
}

fn conflicting_boundary(
    configuration: ToolConfigurationSnapshot,
    script: Vec<Script>,
) -> (
    ToolExecutionBoundary<CountingExecutor, AlwaysConflictCommitter>,
    Arc<Mutex<Vec<AttemptId>>>,
) {
    let dispatches = Arc::new(Mutex::new(Vec::new()));
    let boundary = conflicting_boundary_from(
        &ToolExecutionAssembly::new(&production_root(), configuration),
        script,
        &dispatches,
    );
    (boundary, dispatches)
}

fn sequenced_clock(ticks: Vec<u64>) -> impl FnMut() -> u64 {
    let mut ticks = VecDeque::from(ticks);
    move || ticks.pop_front().expect("clock covers every C-5 reading")
}

fn fixed_clock(timestamp: u64) -> impl FnMut() -> u64 {
    move || timestamp
}

/// Drives one approval-required call whose decision arrives at `decided_at`.
fn approval_call(
    configuration: ToolConfigurationSnapshot,
    turn_deadline: u64,
    decided_at: u64,
) -> (Result<ToolExecutionOutcome, ToolCallError>, usize, u64) {
    let (mut tool_boundary, dispatches) =
        boundary(configuration, vec![Script::Ok(b"ok", EffectState::Started)]);
    let call = inputs(action(Effect::ProcessExecute, "{}"), turn_deadline);
    let mut observed_expiry = 0_u64;
    let result = {
        let capture_expiry = &mut observed_expiry;
        let mut decision = move |request: &ApprovalRequest| {
            *capture_expiry = request.expires_at_millis();
            (ApprovalDecision::Accepted, decided_at)
        };
        // The prepared D-7 record reads the clock between the D-6 creation and
        // the dispatch start, and each audit terminal — the D-6 resolution and
        // the D-7 terminal — reads the clock at its own emission (TC-12/TC-14).
        let mut now = sequenced_clock(vec![
            T0, T0, T0, decided_at, decided_at, decided_at, decided_at,
        ]);
        tool_boundary.execute(&call, &approver(), &mut decision, &mut now)
    };
    (
        result,
        dispatches.lock().expect("log is healthy").len(),
        observed_expiry,
    )
}

fn approval_window_uses_the_earlier_exact_deadline() {
    // Five-minute leg: the Turn deadline is later, so the window is exactly
    // five minutes; a decision at 4:59.999 accepts and dispatches once.
    let later_turn_deadline = T0 + 600_000;
    let (outcome, dispatches, expiry) = approval_call(
        config(Effect::ProcessExecute),
        later_turn_deadline,
        T0 + FIVE_MINUTES - 1,
    );
    assert_eq!(
        outcome.expect("an at-limit decision accepts"),
        ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: EffectState::Started,
        }
    );
    assert_eq!(dispatches, 1);
    assert_eq!(expiry, T0 + FIVE_MINUTES);

    // A decision at exactly 5:00.000 expires: the prepared D-7 is cancelled
    // and the executor is never dispatched.
    let (outcome, dispatches, _) = approval_call(
        config(Effect::ProcessExecute),
        later_turn_deadline,
        T0 + FIVE_MINUTES,
    );
    assert_eq!(
        outcome.expect("an expired approval cancels without an error"),
        ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        }
    );
    assert_eq!(
        dispatches, 0,
        "an expired approval must dispatch zero times"
    );

    // Two-minute leg: the Turn deadline is earlier, so the window is exactly
    // two minutes; a decision at 1:59.999 accepts.
    let early_turn_deadline = T0 + TWO_MINUTES;
    let (outcome, dispatches, expiry) = approval_call(
        config(Effect::ProcessExecute),
        early_turn_deadline,
        T0 + TWO_MINUTES - 1,
    );
    assert_eq!(
        outcome.expect("an at-limit earlier-deadline decision accepts"),
        ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: EffectState::Started,
        }
    );
    assert_eq!(dispatches, 1);
    assert_eq!(expiry, early_turn_deadline);

    // A decision at exactly 2:00.000 expires against the Turn deadline.
    let (outcome, dispatches, _) = approval_call(
        config(Effect::ProcessExecute),
        early_turn_deadline,
        T0 + TWO_MINUTES,
    );
    assert_eq!(
        outcome.expect("the earlier deadline expires the approval"),
        ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        }
    );
    assert_eq!(dispatches, 0);

    // An unscoped principal cannot resolve the request even inside the window.
    let (mut tool_boundary, dispatches) = boundary(
        config(Effect::ProcessExecute),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let call = inputs(action(Effect::ProcessExecute, "{}"), T0 + 600_000);
    let mut decision = |_: &ApprovalRequest| (ApprovalDecision::Accepted, T0 + 1);
    let error = tool_boundary
        .execute(&call, &trust(), &mut decision, &mut fixed_clock(T0))
        .expect_err("a principal without ai.tool.approve cannot decide");
    assert!(
        matches!(error, ToolCallError::Approval(ApprovalError::NotAuthorized)),
        "the unscoped decision must be rejected without mutation: {error:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "an unauthorized decision must dispatch zero times"
    );
}

fn executor_deadline_is_exactly_thirty_seconds() {
    // Completion at started + 29.999s succeeds. The read sequence of the
    // approval-free leg is the durable preparation record, the dispatch plan,
    // the dispatch start, and the deadline check — all at T0 — followed by
    // the post-executor response read and the terminal audit's own
    // observation-time read at T0 + N ms.
    let (mut tool_boundary, dispatches) = boundary(
        config(Effect::ReadData),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut sequenced_clock(vec![T0, T0, T0, T0, T0 + 29_999, T0 + 29_999]),
        )
        .expect("an at-limit duration still commits");
    assert_eq!(
        outcome,
        ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: EffectState::Started,
        }
    );
    assert_eq!(dispatches.lock().expect("log is healthy").len(), 1);

    // Completion at started + 30.000s commits timed_out, not a result.
    let (mut tool_boundary, _) = boundary(
        config(Effect::ReadData),
        vec![Script::Ok(b"late", EffectState::Started)],
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut sequenced_clock(vec![T0, T0, T0, T0, T0 + 30_000, T0 + 30_000]),
        )
        .expect("a deadline crossing still reaches one terminal");
    assert_eq!(
        outcome,
        ToolExecutionOutcome::TimedOut {
            effect_state: EffectState::Started,
        }
    );
}

fn attempt_budget_stops_after_sixteen_attempts() {
    // Slots 1..=16 execute; the 17th allocation is rejected with attempt_limit.
    let script = std::iter::repeat_n(Script::Ok(b"ok", EffectState::Started), 15)
        .chain(std::iter::once(Script::Fail(
            ExecutionFailure::ExecutorUnavailable,
            EffectState::NotStarted,
        )))
        .collect();
    let (mut tool_boundary, dispatches) = boundary(config(Effect::ReadData), script);
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    for slot in 1..=15 {
        let outcome = tool_boundary
            .execute(
                &call,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0),
            )
            .unwrap_or_else(|_| panic!("attempt {slot} must execute"));
        assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));
    }

    // Slot 16 commits a pre-effect failure; the one allowed retry would need a
    // 17th slot, so the action terminates failed/attempt_limit.
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect("an exhausted retry budget still reaches one terminal");
    assert_eq!(
        outcome,
        ToolExecutionOutcome::Failed {
            code: ExecutionFailure::AttemptLimit,
            effect_state: EffectState::NotStarted,
        }
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        16,
        "the rejected retry must not dispatch"
    );

    // Any further allocation for this Turn is rejected with attempt_limit.
    let error = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("the 17th attempt must be rejected");
    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                ExecutionError::AttemptLimit
            ))
        ),
        "the 17th allocation must carry the exact attempt_limit code: {error:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        16,
        "a rejected allocation must not dispatch"
    );
}

fn action_input_cap_is_exact_before_policy_evaluation() {
    // Exactly 65,536 serialized bytes are accepted and still execute.
    let prefix = r#"{"pad":""#;
    let suffix = r#""}"#;
    let padding = MAX_ACTION_INPUT_BYTES - prefix.len() - suffix.len();
    let at_limit = format!("{prefix}{}{suffix}", "a".repeat(padding));
    assert_eq!(at_limit.len(), MAX_ACTION_INPUT_BYTES);
    let parameters = parse_action_parameters(&at_limit)
        .unwrap_or_else(|error| panic!("an at-limit input parses: {error:?}"));
    assert_eq!(
        parameters.canonical().len(),
        MAX_ACTION_INPUT_BYTES,
        "the owned parameters retain the exact at-limit byte count"
    );

    let (mut tool_boundary, _) = boundary(
        config_with_schema(Effect::ReadData, PADDED_SCHEMA),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let call = inputs(
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parameters,
        )
        .expect("valid at-limit action"),
        T0 + 600_000,
    );
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect("an at-limit action still executes");
    assert!(matches!(outcome, ToolExecutionOutcome::Succeeded { .. }));

    // One byte over the cap is rejected with the exact input-limit code
    // before JSON parsing or any policy evaluation.
    let over_limit = format!("{prefix}{}{suffix}", "a".repeat(padding + 1));
    assert_eq!(over_limit.len(), MAX_ACTION_INPUT_BYTES + 1);
    assert_eq!(
        parse_action_parameters(&over_limit),
        Err(ToolAdapterError::InputTooLarge),
        "an over-limit input must return the exact input-limit code"
    );
}

fn executor_output_cap_is_exact() {
    // Exactly 1,048,576 output bytes commit successfully.
    let (mut tool_boundary, _) = boundary(
        config(Effect::ReadData),
        vec![Script::Sized(
            MAX_EXECUTOR_OUTPUT_BYTES,
            EffectState::Started,
        )],
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect("an at-limit output still commits");
    match outcome {
        ToolExecutionOutcome::Succeeded { output, .. } => {
            assert_eq!(output.len(), MAX_EXECUTOR_OUTPUT_BYTES);
        }
        other => panic!("an at-limit output must succeed: {other:?}"),
    }

    // One byte over the cap is discarded and recorded as a typed failure with
    // no payload that could reach the model or history.
    let (mut tool_boundary, dispatches) = boundary(
        config(Effect::ReadData),
        vec![Script::Sized(
            MAX_EXECUTOR_OUTPUT_BYTES + 1,
            EffectState::Started,
        )],
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    let outcome = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect("an over-limit output still reaches one terminal");
    assert_eq!(
        outcome,
        ToolExecutionOutcome::Failed {
            code: ExecutionFailure::OutputLimitExceeded,
            effect_state: EffectState::Started,
        }
    );
    assert_eq!(dispatches.lock().expect("log is healthy").len(), 1);
}

fn one_running_attempt_is_enforced_per_turn() {
    // A first attempt whose terminal commit conflicts stays cataloged as the
    // one running D-7; a second simultaneous action is rejected with the exact
    // concurrent_attempt code and never dispatches.
    let (mut tool_boundary, dispatches) = conflicting_boundary(
        config(Effect::ReadData),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    let conflict = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a conflicting terminal commit owns reconciliation");
    assert!(
        matches!(
            &conflict,
            ToolCallError::Reconciliation(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                ..
            })
        ),
        "the first terminal write conflict must be reconciliation-owned: {conflict:?}"
    );
    assert_eq!(dispatches.lock().expect("log is healthy").len(), 1);

    let concurrent = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a second simultaneous action must be rejected");
    assert!(
        matches!(
            &concurrent,
            ToolCallError::Reconciliation(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::ConcurrentAttempt,
            })
        ),
        "the second simultaneous action must carry the exact concurrent_attempt code: {concurrent:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        1,
        "the running-attempt count never exceeds one"
    );
}

/// One Turn's authority limits are shared across every boundary derived from
/// the same assembly root, so a caller cannot obtain a second 16-slot budget
/// or a second running D-7 by constructing another port-specific boundary
/// (TC-09/TC-12).
fn one_turn_shares_limits_across_boundaries() {
    // A first boundary leaves its D-7 cataloged as the one running attempt; a
    // second boundary for the same Turn is rejected concurrent_attempt and
    // never dispatches through its own executor.
    let assembly = ToolExecutionAssembly::new(&production_root(), config(Effect::ReadData));
    let first_dispatches = Arc::new(Mutex::new(Vec::new()));
    let mut first = conflicting_boundary_from(
        &assembly,
        vec![Script::Ok(b"ok", EffectState::Started)],
        &first_dispatches,
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    assert!(
        first
            .execute(
                &call,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0)
            )
            .is_err(),
        "the conflicting terminal commit owns reconciliation"
    );
    assert_eq!(first_dispatches.lock().expect("log is healthy").len(), 1);

    let second_dispatches = Arc::new(Mutex::new(Vec::new()));
    let mut second = boundary_from(
        &assembly,
        vec![Script::Ok(b"ok", EffectState::Started)],
        &second_dispatches,
    );
    let error = second
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a second boundary shares the same single running slot");
    assert!(
        matches!(
            &error,
            ToolCallError::Reconciliation(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::ConcurrentAttempt,
            })
        ),
        "the shared root must reject the cross-boundary concurrent attempt: {error:?}"
    );
    assert_eq!(
        second_dispatches.lock().expect("log is healthy").len(),
        0,
        "the rejected boundary must not dispatch"
    );

    // A first boundary that exhausts the shared 16-slot budget also exhausts
    // every sibling boundary for the same Turn.
    let assembly = ToolExecutionAssembly::new(&production_root(), config(Effect::ReadData));
    let first_dispatches = Arc::new(Mutex::new(Vec::new()));
    let script = std::iter::repeat_n(Script::Ok(b"ok", EffectState::Started), 16).collect();
    let mut first = boundary_from(&assembly, script, &first_dispatches);
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    for _ in 1..=16 {
        assert!(
            first
                .execute(
                    &call,
                    &trust(),
                    &mut |_| (ApprovalDecision::Accepted, T0),
                    &mut fixed_clock(T0)
                )
                .is_ok(),
            "all sixteen shared slots must be usable from one boundary"
        );
    }
    let second_dispatches = Arc::new(Mutex::new(Vec::new()));
    let mut second = boundary_from(
        &assembly,
        vec![Script::Ok(b"ok", EffectState::Started)],
        &second_dispatches,
    );
    let error = second
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a sibling boundary must see the exhausted shared budget");
    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                ExecutionError::AttemptLimit
            ))
        ),
        "the shared root must reject the cross-boundary 17th attempt: {error:?}"
    );
    assert_eq!(second_dispatches.lock().expect("log is healthy").len(), 0);
}

/// A call whose tenant differs from the authenticated principal is rejected
/// before policy evaluation and D-7 allocation, including on the
/// approval-free `read_data` path, so a caller cannot execute or commit
/// results under another tenant's identity.
fn cross_tenant_call_is_rejected_before_policy() {
    let (mut tool_boundary, dispatches) = boundary(
        config(Effect::ReadData),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let mut foreign = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    foreign.tenant_id = TenantId::new("tenant-b").expect("valid tenant");
    let mut decisions = 0;
    let error = tool_boundary
        .execute(
            &foreign,
            &trust(),
            &mut |_| {
                decisions += 1;
                (ApprovalDecision::Accepted, T0)
            },
            &mut fixed_clock(T0),
        )
        .expect_err("a cross-tenant call must never execute");
    assert!(
        matches!(error, ToolCallError::TenantMismatch),
        "the cross-tenant call must carry the exact tenant-mismatch code: {error:?}"
    );
    assert_eq!(decisions, 0, "no D-6 may be created for a foreign tenant");
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "a cross-tenant read_data call must dispatch zero times"
    );
}

/// A principal without the approval scope never observes the D-6: the
/// decision provider is invoked only after C-7 ownership and scope
/// validation succeeds (TC-05).
fn unauthorized_decision_callback_is_never_invoked() {
    let (mut tool_boundary, dispatches) = boundary(
        config(Effect::ProcessExecute),
        vec![Script::Ok(b"ok", EffectState::Started)],
    );
    let call = inputs(action(Effect::ProcessExecute, "{}"), T0 + 600_000);
    let mut decisions = 0;
    let error = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| {
                decisions += 1;
                (ApprovalDecision::Accepted, T0 + 1)
            },
            &mut fixed_clock(T0),
        )
        .expect_err("an unscoped principal cannot resolve the approval");
    assert!(
        matches!(error, ToolCallError::Approval(ApprovalError::NotAuthorized)),
        "the unscoped decision must be rejected: {error:?}"
    );
    assert_eq!(
        decisions, 0,
        "the decision provider must not observe the D-6 before authorization"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "an unauthorized decision must dispatch zero times"
    );
}

/// One Turn's authority limits are shared across every assembly in the
/// process, so constructing a second assembly cannot reset the 16-slot
/// budget or obtain a second running D-7 (TC-09/TC-12).
fn one_turn_shares_limits_across_assemblies() {
    // A first assembly leaves its D-7 cataloged as the one running attempt; a
    // boundary from a second assembly for the same Turn is rejected
    // concurrent_attempt and never dispatches.
    let first_dispatches = Arc::new(Mutex::new(Vec::new()));
    let root = production_root();
    let first_assembly = ToolExecutionAssembly::new(&root, config(Effect::ReadData));
    let mut first = first_assembly.boundary(
        executor_for(
            vec![Script::Ok(b"ok", EffectState::Started)],
            &first_dispatches,
        ),
        AlwaysCurrentLease,
        AlwaysConflictCommitter,
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    assert!(
        first
            .execute(
                &call,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0)
            )
            .is_err(),
        "the conflicting terminal commit owns reconciliation"
    );
    assert_eq!(first_dispatches.lock().expect("log is healthy").len(), 1);

    let second_dispatches = Arc::new(Mutex::new(Vec::new()));
    let second_assembly = ToolExecutionAssembly::new(&root, config(Effect::ReadData));
    let mut second = second_assembly.boundary(
        executor_for(
            vec![Script::Ok(b"ok", EffectState::Started)],
            &second_dispatches,
        ),
        AlwaysCurrentLease,
        CountingCommitter {
            calls: Arc::new(Mutex::new(0)),
        },
    );
    let error = second
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a second assembly must share the same process authority root");
    assert!(
        matches!(
            &error,
            ToolCallError::Reconciliation(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::ConcurrentAttempt,
            })
        ),
        "the process root must reject the cross-assembly concurrent attempt: {error:?}"
    );
    assert_eq!(
        second_dispatches.lock().expect("log is healthy").len(),
        0,
        "the rejected assembly boundary must not dispatch"
    );
}

/// Two root handles distributed through the production runtime-state access
/// path share one authority catalog: a running D-7 claimed through the first
/// handle rejects a concurrent attempt through the second, so re-entering the
/// runtime state cannot reset the 16-slot budget or the single running slot
/// (TC-09/TC-12).
fn runtime_state_handles_share_one_authority_root() {
    let state = RuntimeState::assemble();
    let first_root = state.tool_execution_root();
    let second_root = state.tool_execution_root();

    // A boundary from the first handle leaves its D-7 cataloged as the one
    // running attempt when its terminal commit conflicts.
    let first_dispatches = Arc::new(Mutex::new(Vec::new()));
    let first_assembly = ToolExecutionAssembly::new(&first_root, config(Effect::ReadData));
    let mut first = first_assembly.boundary(
        executor_for(
            vec![Script::Ok(b"ok", EffectState::Started)],
            &first_dispatches,
        ),
        AlwaysCurrentLease,
        AlwaysConflictCommitter,
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);
    assert!(
        first
            .execute(
                &call,
                &trust(),
                &mut |_| (ApprovalDecision::Accepted, T0),
                &mut fixed_clock(T0)
            )
            .is_err(),
        "the conflicting terminal commit owns reconciliation"
    );
    assert_eq!(first_dispatches.lock().expect("log is healthy").len(), 1);

    // A boundary from the second handle observes the same catalog: the running
    // D-7 rejects the concurrent claim before any executor dispatch.
    let second_dispatches = Arc::new(Mutex::new(Vec::new()));
    let second_assembly = ToolExecutionAssembly::new(&second_root, config(Effect::ReadData));
    let mut second = second_assembly.boundary(
        executor_for(
            vec![Script::Ok(b"ok", EffectState::Started)],
            &second_dispatches,
        ),
        AlwaysCurrentLease,
        CountingCommitter {
            calls: Arc::new(Mutex::new(0)),
        },
    );
    let error = second
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a second runtime-state handle must share the same authority root");
    assert!(
        matches!(
            &error,
            ToolCallError::Reconciliation(ExecutionPending::DispatchRejected {
                code: ExecutionFailure::ConcurrentAttempt,
            })
        ),
        "the runtime state must reject the cross-handle concurrent attempt: {error:?}"
    );
    assert_eq!(
        second_dispatches.lock().expect("log is healthy").len(),
        0,
        "the rejected boundary must not dispatch"
    );
}

/// A lease validator that panics poisons the shared lease lock; every later
/// validation fails closed as fenced instead of reusing the panicked
/// validator's possibly partial state (TC-07).
fn poisoned_lease_validator_fails_closed() {
    struct PanickingLease {
        panicked: std::sync::atomic::AtomicBool,
    }

    impl LeaseValidator for PanickingLease {
        fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
            if !self
                .panicked
                .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("the lease validator failed mid-update");
            }
            LeaseCheck::Current
        }
    }

    let assembly = ToolExecutionAssembly::new(&production_root(), config(Effect::ReadData));
    let dispatches = Arc::new(Mutex::new(Vec::new()));
    let mut tool_boundary = assembly.boundary(
        executor_for(vec![Script::Ok(b"ok", EffectState::Started)], &dispatches),
        PanickingLease {
            panicked: std::sync::atomic::AtomicBool::new(false),
        },
        CountingCommitter {
            calls: Arc::new(Mutex::new(0)),
        },
    );
    let call = inputs(action(Effect::ReadData, "{}"), T0 + 600_000);

    // The first validation panics through the shared lock and poisons it.
    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tool_boundary.execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
    }));
    assert!(first.is_err(), "the fixture panics on the first validation");

    // Every later validation reports typed unavailability — not a disguised
    // fence — and the attempt never dispatches.
    let error = tool_boundary
        .execute(
            &call,
            &trust(),
            &mut |_| (ApprovalDecision::Accepted, T0),
            &mut fixed_clock(T0),
        )
        .expect_err("a poisoned lease validator must never authorize again");
    assert!(
        matches!(
            error,
            ToolCallError::Preparation(ExecutionPreparationError::LeaseUnavailable)
        ),
        "the poisoned validator must report typed lease unavailability, not a fence: {error:?}"
    );
    assert_eq!(
        dispatches.lock().expect("log is healthy").len(),
        0,
        "a poisoned lease must never dispatch"
    );
}

process_local_durable_claims!(CountingCommitter);
process_local_durable_claims!(AlwaysConflictCommitter);

#[test]
fn exact_policy_and_execution_limits() {
    approval_window_uses_the_earlier_exact_deadline();
    executor_deadline_is_exactly_thirty_seconds();
    attempt_budget_stops_after_sixteen_attempts();
    action_input_cap_is_exact_before_policy_evaluation();
    executor_output_cap_is_exact();
    one_running_attempt_is_enforced_per_turn();
    one_turn_shares_limits_across_boundaries();
    one_turn_shares_limits_across_assemblies();
    runtime_state_handles_share_one_authority_root();
    budget::unauthorized_requests_preserve_the_attempt_budget();
    budget::expired_unscoped_requests_preserve_the_attempt_budget();
    cross_tenant_call_is_rejected_before_policy();
    unauthorized_decision_callback_is_never_invoked();
    poisoned_lease_validator_fails_closed();
}
