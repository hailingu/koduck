// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, AttemptCancellationService, AttemptCommitError, AttemptCommitResult,
    AttemptCommitter, CancelAcknowledgement, CancelPermit, CancelledEffectState, DenialCode,
    DispatchPermit, EffectState, ExecutionCoordinator, ExecutionFailure, ExecutionInterrupter,
    ExecutionPending, ExecutionResponse, ExecutionResponseBuilder, ExecutorError,
    InterruptionOutcome, IsolatedExecutor, LeaseCheck, LeaseValidator, PendingApprovalCancellation,
    PendingApprovalCanceller, ToolAuthorizationService, ToolExecutionAuthorityRoot,
    ToolExecutionOutcome, ToolExecutionRuntime, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    ApprovalRequirement, AttemptId, ExactActionBinding, ExecutionAttempt, ExecutionStatus,
    TurnExecutionAuthority,
};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TurnId};

#[path = "cand_2_cancellation_blocking_dispatch.rs"]
mod blocking_dispatch;
#[path = "cand_2_cancellation_disabled_executor.rs"]
mod disabled_executor;
#[path = "cand_2_cancellation_interrupt_barrier.rs"]
mod interrupt_barrier;
#[path = "cand_2_cancellation_interruption_seal.rs"]
mod interruption_seal;
#[path = "cand_2_cancellation_post_claim_lease.rs"]
mod post_claim_lease;
#[path = "cand_2_cancellation_pre_dispatch.rs"]
mod pre_dispatch;
#[path = "cand_2_cancellation_transport.rs"]
mod transport;
#[path = "cand_2_cancellation_turn_terminal.rs"]
mod turn_terminal;

/// Executor that records dispatches and plays one scripted cancellation.
struct CancellingExecutor {
    dispatches: usize,
    cancels: usize,
    authority_observed_during_cancellation: Option<TurnExecutionAuthority>,
    cancellation_observed_terminal_reservation: Option<bool>,
    acknowledgement: CancelAcknowledgement,
    response: Result<ExecutionResponse, ExecutorError>,
    execution_deadlines: Vec<u64>,
    cancellation_deadlines: Vec<u64>,
}

/// Executor that holds dispatch open until the test permits it to return.
struct BlockingExecutor {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl IsolatedExecutor for BlockingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.entered.send(()).expect("test observes dispatch entry");
        self.release.recv().expect("test releases blocked dispatch");
        Ok(response(EffectState::NotStarted, b"result"))
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::Acknowledged(CancelledEffectState::NotStarted)
    }
}

/// Cancellation service that announces the post-seal prepared close and holds
/// it open until the test releases it.
///
/// `request_interruption` seals the Turn before any cancellation runs, so the
/// entry signal deterministically proves the sealed-but-still-Prepared window
/// that a fixed sleep can only approximate.
struct SignallingPreparedCancellation {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    inner: ExecutionCoordinator<CancellingExecutor, AlwaysCurrentLease, WinningCommitter>,
}

impl AttemptCancellationService for SignallingPreparedCancellation {
    fn cancel_prepared(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        self.entered
            .send(())
            .expect("test observes the post-seal prepared close");
        self.release
            .recv()
            .expect("test releases the blocked prepared close");
        self.inner.cancel_prepared(authority, attempt)
    }

    fn cancel_running(
        &mut self,
        authority: &mut TurnExecutionAuthority,
        attempt: &mut ExecutionAttempt,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ExecutionPending> {
        self.inner.cancel_running(authority, attempt, now)
    }
}

impl IsolatedExecutor for CancellingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        _binding: &ExactActionBinding,
        deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.dispatches += 1;
        self.execution_deadlines.push(deadline.remaining_millis());
        self.response.clone()
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        self.cancels += 1;
        self.cancellation_observed_terminal_reservation = self
            .authority_observed_during_cancellation
            .as_ref()
            .map(|authority| authority.live_attempts().is_empty());
        self.cancellation_deadlines
            .push(deadline.remaining_millis());
        self.acknowledgement
    }
}

struct AlwaysCurrentLease;

impl LeaseValidator for AlwaysCurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Lease that plays a fixed sequence of decisions.
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

struct WinningCommitter {
    calls: usize,
}

/// Committer that plays a scripted sequence of commit results, one per call.
struct SequencedCommitter {
    calls: usize,
    results: VecDeque<Result<AttemptCommitResult, AttemptCommitError>>,
}

impl AttemptCommitter for SequencedCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.calls += 1;
        self.results
            .pop_front()
            .expect("sequenced committer has a result for every call")
    }
}

/// Committer whose conditional terminal write always fails as unavailable.
struct UnavailableCommitter {
    calls: usize,
}

impl AttemptCommitter for UnavailableCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.calls += 1;
        Err(AttemptCommitError::Unavailable)
    }
}

struct NoPendingApprovals;

impl PendingApprovalCanceller for NoPendingApprovals {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        Ok(PendingApprovalCancellation::Cancelled)
    }
}

/// Approval port representing an already-accepted D-6 for the same D-7.
struct AlreadyAcceptedApproval;

impl PendingApprovalCanceller for AlreadyAcceptedApproval {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        Ok(PendingApprovalCancellation::AlreadyResolved)
    }
}

#[derive(Default)]
struct RecordingPendingApprovals {
    cancelled_attempts: Vec<AttemptId>,
}

impl PendingApprovalCanceller for RecordingPendingApprovals {
    fn cancel_requested(
        &mut self,
        binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        self.cancelled_attempts.push(binding.attempt_id());
        Ok(PendingApprovalCancellation::Cancelled)
    }
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

/// One Turn execution context whose catalog, preparer, and interrupter share
/// the same process-owned authority root.
struct Harness {
    runtime: ToolExecutionRuntime,
    tenant: TenantId,
    thread: ThreadId,
    turn: TurnId,
}

impl Harness {
    fn new() -> Self {
        Self {
            runtime: ToolExecutionRuntime::new(&ToolExecutionAuthorityRoot::new()),
            tenant: TenantId::new("tenant-a").expect("valid tenant"),
            thread: ThreadId::new(),
            turn: TurnId::new(),
        }
    }

    /// Binds a harness to the production runtime's already-issued C-5 root.
    fn with_runtime(
        runtime: ToolExecutionRuntime,
        tenant: TenantId,
        thread: ThreadId,
        turn: TurnId,
    ) -> Self {
        Self {
            runtime,
            tenant,
            thread,
            turn,
        }
    }

    /// Prepares one fresh in-profile read-only D-7 identity for this Turn.
    fn prepared(&self) -> (TurnExecutionAuthority, ExecutionAttempt) {
        let binding = sealed_binding(self.tenant.clone(), self.thread, self.turn);
        let mut preparer = self.runtime.preparer(AlwaysCurrentLease);
        preparer
            .prepare(binding)
            .expect("current owner has an attempt slot")
    }

    /// Prepares one D-7 with a canonical D-6 requirement for interruption tests.
    fn approval_required_prepared(&self) -> (TurnExecutionAuthority, ExecutionAttempt) {
        let mut binding = sealed_binding(self.tenant.clone(), self.thread, self.turn);
        binding.authorize_policy(ApprovalRequirement::Required);
        let mut preparer = self.runtime.preparer(AlwaysCurrentLease);
        preparer
            .prepare(binding)
            .expect("current owner has an approval-required attempt slot")
    }

    fn running(&self, started_at_millis: u64) -> (TurnExecutionAuthority, ExecutionAttempt) {
        let (mut authority, mut attempt) = self.prepared();
        authority
            .claim_dispatch(&mut attempt, None, started_at_millis)
            .expect("in-profile read-only action dispatches without approval");
        (authority, attempt)
    }

    fn interrupter(&self) -> ExecutionInterrupter {
        self.runtime.interrupter()
    }
}

fn sealed_binding(tenant: TenantId, thread: ThreadId, turn: TurnId) -> ExactActionBinding {
    let binding = ExactActionBinding::new(
        tenant,
        thread,
        turn,
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters("{}").expect("valid parameters"),
        )
        .expect("valid action"),
    )
    .expect("valid binding");
    authorize(binding).expect("fixture read-only action is policy-authorized")
}

fn authorize(binding: ExactActionBinding) -> Result<ExactActionBinding, DenialCode> {
    let descriptor = CapabilityDescriptor::new(
        "fixture.tool",
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
        .allow("fixture.tool", "v1", Effect::ReadData, "fixture-target")
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

fn executor(acknowledgement: CancelAcknowledgement) -> CancellingExecutor {
    CancellingExecutor {
        dispatches: 0,
        cancels: 0,
        authority_observed_during_cancellation: None,
        cancellation_observed_terminal_reservation: None,
        acknowledgement,
        response: Ok(response(EffectState::NotStarted, b"result")),
        execution_deadlines: Vec::new(),
        cancellation_deadlines: Vec::new(),
    }
}

fn response(effect_state: EffectState, output: &[u8]) -> ExecutionResponse {
    let mut response = ExecutionResponseBuilder::new(effect_state);
    response
        .push_chunk(output)
        .expect("fixture response is within the output limit");
    response.finish().expect("fixture response remains bounded")
}

fn coordinator(
    executor: CancellingExecutor,
) -> ExecutionCoordinator<CancellingExecutor, AlwaysCurrentLease, WinningCommitter> {
    ExecutionCoordinator::new(executor, AlwaysCurrentLease, WinningCommitter { calls: 0 })
}

#[test]
fn interrupting_a_prepared_attempt_dispatches_nothing() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 1);
    // The closed Turn has no live D-7 left to interrupt.
    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::NoLiveAttempt)
    );
}

#[test]
fn interruption_closes_the_requested_approval_for_its_exact_prepared_attempt() {
    let harness = Harness::new();
    let (_authority, attempt) = harness.approval_required_prepared();
    let attempt_id = attempt.binding().attempt_id();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    let mut approvals = RecordingPendingApprovals::default();

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut approvals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert_eq!(approvals.cancelled_attempts, vec![attempt_id]);
    assert_eq!(coordinator.executor().dispatches, 0);
}

#[test]
fn interruption_cancels_prepared_d7_after_its_d6_was_accepted() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.approval_required_prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut AlreadyAcceptedApproval,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn interruption_closes_every_prepared_attempt_for_the_turn() {
    let harness = Harness::new();
    let (authority, _first_attempt) = harness.prepared();
    let (_second_authority, _second_attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert!(matches!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::ClosedMany(outcomes))
            if outcomes == [
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                },
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                },
            ]
    ));
    assert!(authority.live_attempts().is_empty());
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 2);
}

#[test]
fn mid_loop_cancellation_failure_returns_partial_close_results() {
    let harness = Harness::new();
    let (_authority, _first) = harness.prepared();
    let (_second_authority, _second) = harness.prepared();
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        AlwaysCurrentLease,
        SequencedCommitter {
            calls: 0,
            results: VecDeque::from([
                Ok(AttemptCommitResult::Won),
                Err(AttemptCommitError::Unavailable),
            ]),
        },
    );

    let result = harness.interrupter().interrupt(
        &mut coordinator,
        &mut NoPendingApprovals,
        &harness.tenant,
        harness.thread,
        harness.turn,
        &mut || 1_000,
    );

    // The first D-7 durably closed; the second failed. The caller must observe
    // the partial close with the failure reason rather than a misleading total
    // success that hides the first permanent closure or the remaining live D-7.
    assert!(matches!(
        result,
        Ok(InterruptionOutcome::PartiallyClosed { closed, pending })
            if closed.len() == 1
                && matches!(pending, ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    ..
                })
    ));
    assert_eq!(coordinator.committer().calls, 2);
}

#[test]
fn stale_prepared_snapshot_cannot_durably_cancel_after_dispatch() {
    let harness = Harness::new();
    let (mut authority, mut running_attempt) = harness.prepared();
    let mut stale_prepared_attempt = authority
        .live_attempts()
        .pop()
        .expect("prepared attempt is cataloged");
    authority
        .claim_dispatch(&mut running_attempt, None, 1_000)
        .expect("current attempt enters running before the stale cancellation commits");
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert!(
        coordinator
            .cancel_prepared_attempt(&mut authority, &mut stale_prepared_attempt)
            .is_err()
    );
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(running_attempt.status(), ExecutionStatus::Running);
}

#[test]
fn interruption_seal_blocks_racing_dispatch_and_closes_prepared_d7() {
    let harness = Harness::new();
    let (authority, _attempt) = harness.prepared();

    // The interruption seals the Turn before any racing dispatcher can
    // claim_dispatch. The seal wins under the authority lock: claim_dispatch
    // returns InterruptionRequested, and the interruption closes the prepared
    // D-7 through the normal cancellation path without executor dispatch.
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert!(authority.live_attempts().is_empty());
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn interrupting_an_unknown_turn_reports_no_live_attempt() {
    let harness = Harness::new();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &TenantId::new("tenant-b").expect("valid tenant"),
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::NoLiveAttempt)
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
}

#[test]
fn acknowledged_not_started_cancellation_commits_cancelled() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn running_cancellation_claims_terminal_before_executor_cancel() {
    let harness = Harness::new();
    let (authority, _attempt) = harness.running(1_000);
    let mut cancellation_executor = executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    ));
    cancellation_executor.authority_observed_during_cancellation = Some(authority.new_handle());
    let mut coordinator = coordinator(cancellation_executor);

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            }
        ))
    );
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(
        coordinator
            .executor()
            .cancellation_observed_terminal_reservation,
        Some(true),
        "the D-7 must be unavailable to a competing interrupter before cancellation dispatch",
    );
}

#[test]
fn terminal_commit_in_flight_requires_reconciliation_not_no_live_attempt() {
    let harness = Harness::new();
    let (mut authority, attempt) = harness.running(1_000);
    authority
        .reserve_terminal(&attempt)
        .expect("fixture reserves the running D-7 while a durable terminal is in flight");
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
}

#[test]
fn in_flight_terminal_prevents_partial_interruption_of_other_live_attempts() {
    let harness = Harness::new();
    let (mut authority, running_attempt) = harness.running(1_000);
    let (_second_authority, prepared_attempt) = harness.prepared();
    authority
        .reserve_terminal(&running_attempt)
        .expect("fixture reserves the running D-7 while its terminal is in flight");
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(prepared_attempt.status(), ExecutionStatus::Prepared);
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
}

#[test]
fn post_cancel_fence_keeps_the_running_attempt_reserved_for_reconciliation() {
    let harness = Harness::new();
    let (authority, _attempt) = harness.running(1_000);
    let mut first_cancellation = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::NotStarted,
        )),
        SequencedLease {
            decisions: VecDeque::from([true, false]),
        },
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut first_cancellation,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(first_cancellation.executor().cancels, 1);
    assert!(
        authority.live_attempts().is_empty(),
        "a sent cancellation must keep its D-7 unavailable to another interrupter",
    );

    let mut later_interrupter = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    assert_eq!(
        harness.interrupter().interrupt(
            &mut later_interrupter,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(later_interrupter.executor().cancels, 0);
    assert_eq!(later_interrupter.committer().calls, 0);
}

#[test]
fn post_dispatch_durability_failure_keeps_the_running_attempt_reserved_against_interruption() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    // The dispatch reaches the executor, so an external effect was requested,
    // but the canonical terminal write fails. The running D-7 must stay reserved
    // so a repeated interruption cannot cancel it and commit a contradictory
    // terminal.
    let mut dispatch = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::Started,
        )),
        AlwaysCurrentLease,
        UnavailableCommitter { calls: 0 },
    );

    assert_eq!(
        dispatch.execute(&mut authority, None, &mut attempt, 1_000, &mut || 1_000),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(dispatch.executor().dispatches, 1);
    assert_eq!(dispatch.committer().calls, 1);
    assert!(
        authority.live_attempts().is_empty(),
        "a running D-7 whose post-dispatch terminal write did not win must stay reserved until reconciliation",
    );

    let mut later = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    assert_eq!(
        harness.interrupter().interrupt(
            &mut later,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(later.executor().cancels, 0);
    assert_eq!(later.committer().calls, 0);
}

#[test]
fn acknowledged_started_cancellation_reports_the_started_effect() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::Started,
            }
        ))
    );
    assert_eq!(coordinator.executor().cancels, 1);
}

#[test]
fn acknowledged_cancellation_only_commits_defined_cancelled_terminals() {
    // The acknowledgement type carries only NotStarted or Started, so an
    // acknowledged cancellation commits a defined `cancelled/not_started` or
    // `cancelled/started` terminal; `unknown` is reserved for the unacknowledged
    // `timed_out` path and cannot reach a `cancelled` terminal.
    let harness = Harness::new();

    let (mut authority, mut attempt) = harness.running(1_000);
    let mut not_started = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    assert_eq!(
        not_started.cancel_running_attempt(&mut authority, &mut attempt, &mut || 1_000),
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::NotStarted,
        })
    );

    let (mut authority, mut attempt) = harness.running(1_000);
    let mut started = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));
    assert_eq!(
        started.cancel_running_attempt(&mut authority, &mut attempt, &mut || 1_000),
        Ok(ToolExecutionOutcome::Cancelled {
            effect_state: EffectState::Started,
        })
    );
}

#[test]
fn unacknowledged_cancellation_times_out_with_unknown_effect() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::TimedOut {
                effect_state: EffectState::Unknown,
            }
        ))
    );
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
    assert_eq!(coordinator.executor().cancellation_deadlines, vec![30_000]);
}

#[test]
fn cancellation_at_the_action_deadline_times_out_without_an_executor_call() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));

    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut attempt, &mut || 31_000),
        Ok(ToolExecutionOutcome::TimedOut {
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn late_cancellation_acknowledgement_commits_timeout_with_unknown_effect() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));

    // The cancellation is sent before the deadline (observed at 6s, 25s left),
    // but the bounded executor acknowledgement arrives after the 30-second
    // action deadline; the deadline dominates and the D-7 commits
    // `timed_out/unknown` rather than `cancelled`.
    let mut calls = 0;
    let mut now = || {
        calls += 1;
        if calls == 1 { 6_000 } else { 31_001 }
    };
    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut attempt, &mut now),
        Ok(ToolExecutionOutcome::TimedOut {
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 1);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn fenced_owner_interruption_requires_reconciliation() {
    let harness = Harness::new();
    let (retained_authority, _attempt) = harness.running(1_000);
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::Acknowledged(
            CancelledEffectState::NotStarted,
        )),
        SequencedLease {
            decisions: VecDeque::from([false]),
        },
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state: EffectState::Unknown,
        })
    );
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
    // The fenced running D-7 is still the cataloged live attempt.
    assert_eq!(
        retained_authority
            .live_attempts()
            .first()
            .map(ExecutionAttempt::status),
        Some(ExecutionStatus::Running)
    );
}

#[test]
fn fenced_owner_cannot_commit_a_prepared_attempt_cancellation() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.prepared();
    let mut coordinator = ExecutionCoordinator::new(
        executor(CancelAcknowledgement::NotAcknowledged),
        SequencedLease {
            decisions: VecDeque::from([false]),
        },
        WinningCommitter { calls: 0 },
    );

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::OwnerFencedBeforeDispatch,
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
}

#[test]
fn deadline_crossing_commits_timeout_instead_of_success() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));

    // One millisecond inside the 30-second action deadline still succeeds.
    let mut before_deadline_clock = VecDeque::from([1_000, 30_999]);
    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || {
            before_deadline_clock
                .pop_front()
                .expect("clock supplies dispatch and result")
        },),
        Ok(ToolExecutionOutcome::Succeeded {
            output: b"result".to_vec(),
            effect_state: EffectState::NotStarted,
        })
    );

    // At exactly 30 seconds elapsed the deadline dominates: the executor's
    // bounded output is discarded and the D-7 commits timed_out.
    let (mut authority, mut attempt) = harness.prepared();
    let mut at_deadline_clock = VecDeque::from([1_000, 31_000]);
    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || {
            at_deadline_clock
                .pop_front()
                .expect("clock supplies dispatch and result")
        },),
        Ok(ToolExecutionOutcome::TimedOut {
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 2);
    assert_eq!(coordinator.committer().calls, 2);
    assert_eq!(
        coordinator.executor().execution_deadlines,
        vec![30_000, 30_000]
    );
}

#[test]
fn exhausted_action_budget_times_out_without_executor_dispatch() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    let mut clock = VecDeque::from([31_000, 31_000]);

    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 1_000, &mut || {
            clock
                .pop_front()
                .expect("clock supplies dispatch and terminal observations")
        }),
        Ok(ToolExecutionOutcome::TimedOut {
            effect_state: EffectState::NotStarted,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
}

#[test]
fn cancelled_attempt_rejects_late_result_delivery() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.running(1_000);
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::Started,
    )));

    assert_eq!(
        harness.interrupter().interrupt(
            &mut coordinator,
            &mut NoPendingApprovals,
            &harness.tenant,
            harness.thread,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(InterruptionOutcome::Closed(
            ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::Started,
            }
        ))
    );
    assert_eq!(
        coordinator.execute(&mut authority, None, &mut attempt, 2_000, &mut || 2_000),
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::ApprovalAlreadyConsumed,
        })
    );
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 1);
    assert!(authority.live_attempts().is_empty());
}

#[test]
fn cancelling_a_non_running_attempt_is_rejected_without_executor_calls() {
    let harness = Harness::new();
    let (mut authority, mut attempt) = harness.prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));

    assert_eq!(
        coordinator.cancel_running_attempt(&mut authority, &mut attempt, &mut || 1_000),
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::AttemptNotRunning,
        })
    );
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.committer().calls, 0);
    assert_eq!(attempt.status(), ExecutionStatus::Prepared);
}
