// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Coordinator durable-gate legs: identity-conflicted preparation cleanup,
//! fenced durable claims, and concurrent claims whose progressed rows must
//! keep their canonical state (ADR-0003 TC-10/TC-12).

use std::collections::VecDeque;

use koduck_ai::application::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, AttemptInsertResolution,
    AttemptStoreError, DispatchClaimResolution, DurableAttemptTransitions, EffectState,
    ExecutionCoordinator, ExecutionFailure, ExecutionPending, ExecutionPreparationError,
    ExecutorError, ToolCallError, ToolCallInputs, ToolConfigurationSnapshot, ToolExecutionAssembly,
};
use koduck_ai::domain::execution::ExecutionStatus;
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::ToolExecutionOutcome;
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalRequest, ExactActionBinding, ExecutionError,
};

use super::{RecordingExecutor, SequencedLease, response};

/// Read-data snapshot so the driver's policy seal requires no D-6.
fn read_data_snapshot() -> ToolConfigurationSnapshot {
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
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
    snapshot
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.tool", "v1", Effect::ReadData, "fixture-target")
                .expect("valid profile entry")
                .build(),
        )
        .expect("profile registers");
    snapshot
}

/// Committer double whose durable prepared insert always reports an identity
/// conflict, proving the binding never committed canonically under this exact
/// identity.
struct IdentityConflictingCommitter {
    commits: usize,
}

/// Pending-approval double for the interruption leg: this fixture's read-data
/// D-7 requires no D-6, so the canceller is never consulted.
struct NoApprovalsNeeded;

impl koduck_ai::application::PendingApprovalCanceller for NoApprovalsNeeded {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PendingApprovalCancellation, ExecutionPending> {
        unreachable!("a read-data preparation has no requested D-6 to cancel")
    }
}

/// Double whose durable claim always reports a concurrent slot owner and
/// whose prepared-only close then observes the row progressed under another
/// claimant: the coordinator must defer to reconciliation without dispatch.
struct ConcurrentProgressedCommitter;

/// Double that proves the durable 16-attempt cap independently of the local
/// authority budget, as after a process restart (ADR-0003 TC-09/TC-12).
struct DurableLimitCommitter;

/// Durable-transition double that allows one pre-effect execution before its
/// retry exhausts the canonical attempt budget.
struct RetryLimitCommitter {
    inserts: VecDeque<Result<AttemptInsertResolution, AttemptStoreError>>,
}

/// Durable double whose prepared replay proves another instance already owns
/// this exact D-7 in its running state.
struct ProgressedPreparedCommitter;

/// Durable double whose prepared-insert acknowledgement is unavailable after
/// the canonical write may already have committed.
struct UnavailablePreparedCommitter;

impl AttemptCommitter for ConcurrentProgressedCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        unreachable!("a rejected claim never reaches a terminal commit")
    }
}

impl AttemptCommitter for ProgressedPreparedCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        unreachable!("a progressed prepared replay never commits locally")
    }
}

impl AttemptCommitter for UnavailablePreparedCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        unreachable!("an unavailable prepared insert never commits locally")
    }
}

fn read_data_inputs(tenant_id: TenantId, thread_id: ThreadId, turn_id: TurnId) -> ToolCallInputs {
    ToolCallInputs {
        tenant_id,
        thread_id,
        turn_id,
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    }
}

#[test]
fn progressed_prepared_replay_reports_unknown_effect_evidence() {
    // A replay can observe the same D-7 already running under another process.
    // Its effect evidence is therefore unknown, not proof that no effect began
    // (ADR-0003 TC-10/TC-12).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let inputs = read_data_inputs(tenant, ThreadId::new(), TurnId::new());
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        ProgressedPreparedCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;

    assert!(matches!(
        boundary.execute(&inputs, &trust, &mut decision, &mut now),
        Err(ToolCallError::Reconciliation(
            ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            }
        ))
    ));
}

#[test]
fn progressed_prepared_replay_stays_reserved_for_interruption_reconciliation() {
    // A replay that observes the canonical identity already running elsewhere
    // cannot be cancelled through this stale local Prepared mirror. It must
    // retain its terminal reservation, so a later interruption reconciles
    // instead of issuing another prepared-only close (ADR-0003 TC-10/TC-12).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let inputs = read_data_inputs(tenant.clone(), thread_id, turn_id);
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        ProgressedPreparedCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;

    let result = boundary.execute(&inputs, &trust, &mut decision, &mut now);
    assert!(matches!(
        result,
        Err(ToolCallError::Reconciliation(
            ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::TerminalConflict,
                effect_state: EffectState::Unknown,
            }
        ))
    ));

    let mut cancellation = ExecutionCoordinator::new(
        koduck_ai::adapters::execution::DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        ProgressedPreparedCommitter,
    );
    let mut approvals = NoApprovalsNeeded;
    assert_eq!(
        root.runtime().interrupter().interrupt(
            &mut cancellation,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &tenant,
            thread_id,
            turn_id,
            &mut now,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        }),
        "the stale local attempt is reserved while the progressed canonical row is reconciled"
    );
}

#[test]
fn unavailable_prepared_insert_reports_unknown_effect_evidence() {
    // A timed-out durable insert may have committed before its acknowledgement
    // was lost, so another instance can progress the D-7 before reconciliation
    // observes it (ADR-0003 TC-08/TC-12).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let inputs = read_data_inputs(tenant, ThreadId::new(), TurnId::new());
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        UnavailablePreparedCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;

    assert!(matches!(
        boundary.execute(&inputs, &trust, &mut decision, &mut now),
        Err(ToolCallError::Reconciliation(
            ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::DurabilityUnavailable,
                effect_state: EffectState::Unknown,
            }
        ))
    ));
}

#[test]
fn identity_conflicted_preparation_leaves_no_orphan_live_attempt() {
    // A durable identity conflict proves this binding never committed
    // canonically, so the local D-7 can neither dispatch nor terminalize: C-5
    // must close it cancelled before any effect instead of leaving orphan
    // live work that a later interruption has to reconcile (ADR-0003
    // TC-12/TC-13).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let inputs = ToolCallInputs {
        tenant_id: tenant.clone(),
        thread_id,
        turn_id,
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    };
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let committer = IdentityConflictingCommitter { commits: 0 };
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"ok")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        committer,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;
    let error = boundary
        .execute(&inputs, &trust, &mut decision, &mut now)
        .expect_err("a conflicted preparation fails the call closed");
    assert!(
        matches!(
            error,
            koduck_ai::application::ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    ..
                }
            )
        ),
        "the conflict surfaces as reconciliation, found {error:?}"
    );
    // The conflicted D-7 is closed locally: a later interruption of the same
    // Turn observes no live attempt to cancel or reconcile.
    let mut coordinator = ExecutionCoordinator::new(
        koduck_ai::adapters::execution::DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        IdentityConflictingCommitter { commits: 0 },
    );
    let mut approvals = NoApprovalsNeeded;
    let outcome = root.runtime().interrupter().interrupt(
        &mut coordinator,
        &mut koduck_ai::application::NoToolAudits,
        &mut approvals,
        &tenant,
        thread_id,
        turn_id,
        &mut now,
    );
    assert_eq!(
        outcome,
        Ok(koduck_ai::application::InterruptionOutcome::NoLiveAttempt),
        "no orphan live attempt remains after the conflicted preparation"
    );
}

#[test]
fn concurrent_claim_with_a_progressed_close_defers_to_reconciliation() {
    // The durable slot is owned by another D-7 and a racing claimant already
    // progressed this exact identity: the coordinator must neither dispatch
    // nor rewrite the progressed row — reconciliation owns the next
    // transition (ADR-0003 TC-10/TC-12).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let inputs = ToolCallInputs {
        tenant_id: tenant.clone(),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    };
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"ok")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, true]),
        },
        ConcurrentProgressedCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;
    let error = boundary
        .execute(&inputs, &trust, &mut decision, &mut now)
        .expect_err("a progressed close defers to reconciliation");
    assert!(
        matches!(
            error,
            koduck_ai::application::ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    effect_state: EffectState::Unknown,
                }
            )
        ),
        "the progressed row keeps its canonical state, found {error:?}"
    );
    // This local attempt was marked Running before the durable claim lost. It
    // was never dispatched by this coordinator, so its held terminal
    // reservation must prevent an interruption from issuing a cancellation to
    // an executor owned by the competing claimant.
    let mut cancellation = ExecutionCoordinator::new(
        koduck_ai::adapters::execution::DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        ConcurrentProgressedCommitter,
    );
    let mut approvals = NoApprovalsNeeded;
    assert_eq!(
        root.runtime().interrupter().interrupt(
            &mut cancellation,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &tenant,
            inputs.thread_id,
            inputs.turn_id,
            &mut now,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        }),
        "a concurrent claimant's running identity remains reserved for reconciliation"
    );
}

#[test]
fn concurrent_claim_fenced_before_prepared_close_retains_unknown_reconciliation() {
    // The process-local claim has already marked this D-7 Running when the
    // durable slot is lost. A subsequent lease fence cannot prove the
    // canonical effect stayed not_started, and must retain the local terminal
    // reservation so interruption cannot cancel work this coordinator never
    // dispatched (ADR-0003 TC-07/TC-10/TC-12).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let inputs = read_data_inputs(tenant.clone(), thread_id, turn_id);
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"unused")),
        },
        SequencedLease {
            decisions: VecDeque::from([true, true, false]),
        },
        ConcurrentProgressedCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;

    assert!(matches!(
        boundary.execute(&inputs, &trust, &mut decision, &mut now),
        Err(ToolCallError::Reconciliation(
            ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedBeforeDispatch,
                effect_state: EffectState::Unknown,
            }
        ))
    ));

    let mut cancellation = ExecutionCoordinator::new(
        koduck_ai::adapters::execution::DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        ConcurrentProgressedCommitter,
    );
    let mut approvals = NoApprovalsNeeded;
    assert_eq!(
        root.runtime().interrupter().interrupt(
            &mut cancellation,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &tenant,
            thread_id,
            turn_id,
            &mut now,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::TerminalConflict,
            effect_state: EffectState::Unknown,
        }),
        "the fenced claim loser remains reserved for durable reconciliation"
    );
}

#[test]
fn durable_attempt_budget_exhaustion_is_the_exact_attempt_limit_rejection() {
    // A fresh local authority can be created after process restart while the
    // canonical Turn already holds 16 rows. The durable cap is therefore a
    // normal attempt_limit rejection, not a durability outage (ADR-0003
    // TC-09).
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let inputs = ToolCallInputs {
        tenant_id: tenant,
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    };
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"ok")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        DurableLimitCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;
    assert!(
        matches!(
            boundary.execute(&inputs, &trust, &mut decision, &mut now),
            Err(ToolCallError::Preparation(
                ExecutionPreparationError::Rejected(ExecutionError::AttemptLimit)
            ))
        ),
        "the durable cap must use the same exact rejection as the local cap"
    );
}

#[test]
fn durable_attempt_limit_leaves_no_orphan_local_attempt() {
    // A durable cap is definitive evidence that this newly allocated local
    // D-7 has no canonical row. The local catalog must therefore close it,
    // rather than making a later interruption reconcile an invisible attempt.
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let inputs = ToolCallInputs {
        tenant_id: tenant.clone(),
        thread_id,
        turn_id,
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    };
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Ok(response(EffectState::NotStarted, b"ok")),
        },
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        DurableLimitCommitter,
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;
    assert!(matches!(
        boundary.execute(&inputs, &trust, &mut decision, &mut now),
        Err(ToolCallError::Preparation(
            ExecutionPreparationError::Rejected(ExecutionError::AttemptLimit)
        ))
    ));

    let mut cancellation = ExecutionCoordinator::new(
        koduck_ai::adapters::execution::DisabledExecutor,
        SequencedLease {
            decisions: VecDeque::from([true]),
        },
        DurableLimitCommitter,
    );
    let mut approvals = NoApprovalsNeeded;
    assert_eq!(
        root.runtime().interrupter().interrupt(
            &mut cancellation,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &tenant,
            thread_id,
            turn_id,
            &mut now,
        ),
        Ok(koduck_ai::application::InterruptionOutcome::NoLiveAttempt),
        "a definitive durable rejection must not leave local live work"
    );
}

#[test]
fn durable_retry_budget_exhaustion_returns_the_terminal_attempt_limit_outcome() {
    // When attempt 16 commits a proven pre-effect failure, the retry's durable
    // allocation failure is the same bounded outcome as a local slot-17
    // rejection: it must become failed/attempt_limit, not a preparation error.
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust = TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let inputs = ToolCallInputs {
        tenant_id: tenant,
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
        profile_id: String::from("profile-default"),
        profile_version: String::from("v1"),
        action: Action::new(
            "fixture.tool",
            "v1",
            Effect::ReadData,
            "fixture-target",
            parse_action_parameters(r"{}").expect("valid parameters"),
        )
        .expect("valid action"),
        turn_deadline_millis: u64::MAX,
    };
    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, read_data_snapshot());
    let mut boundary = assembly.boundary(
        RecordingExecutor {
            calls: 0,
            response: Err(ExecutorError::new(
                ExecutionFailure::ExecutorUnavailable,
                EffectState::NotStarted,
            )),
        },
        SequencedLease {
            decisions: VecDeque::from([true; 8]),
        },
        RetryLimitCommitter {
            inserts: VecDeque::from([
                Ok(AttemptInsertResolution::Inserted),
                Err(AttemptStoreError::AttemptLimit),
            ]),
        },
    );
    let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0_u64);
    let mut now = || 1_000_u64;
    assert!(
        matches!(
            boundary.execute(&inputs, &trust, &mut decision, &mut now),
            Ok(ToolExecutionOutcome::Failed {
                code: ExecutionFailure::AttemptLimit,
                effect_state: EffectState::NotStarted,
            })
        ),
        "a durable retry cap must produce the declared final attempt-limit outcome"
    );
}
impl AttemptCommitter for IdentityConflictingCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.commits += 1;
        Ok(AttemptCommitResult::Won)
    }
}

impl DurableAttemptTransitions for IdentityConflictingCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Err(AttemptStoreError::IdentityConflict)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        unreachable!("a conflicted preparation never claims a dispatch")
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        unreachable!("a conflicted preparation never reaches a close")
    }
}

impl DurableAttemptTransitions for ProgressedPreparedCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Ok(AttemptInsertResolution::Existing {
            status: ExecutionStatus::Running,
            version: 2,
        })
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        unreachable!("a progressed prepared replay never claims a dispatch")
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        panic!("a progressed prepared replay must remain reserved and never close locally")
    }
}

impl DurableAttemptTransitions for UnavailablePreparedCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Err(AttemptStoreError::Unavailable)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        unreachable!("an unavailable prepared insert never claims a dispatch")
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        unreachable!("an unavailable prepared insert never closes locally")
    }
}

impl DurableAttemptTransitions for ConcurrentProgressedCommitter {
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
        Ok(DispatchClaimResolution::Concurrent)
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        Ok(
            koduck_ai::application::PreparedCloseResolution::Progressed {
                status: ExecutionStatus::Running,
                version: 2,
            },
        )
    }
}

impl AttemptCommitter for DurableLimitCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        unreachable!("a durable attempt cap rejects before dispatch")
    }
}

impl DurableAttemptTransitions for DurableLimitCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Err(AttemptStoreError::AttemptLimit)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        unreachable!("a durable attempt cap rejects before a dispatch claim")
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<koduck_ai::application::PreparedCloseResolution, AttemptStoreError> {
        Err(AttemptStoreError::Unavailable)
    }
}

impl AttemptCommitter for RetryLimitCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Won)
    }
}

impl DurableAttemptTransitions for RetryLimitCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        self.inserts
            .pop_front()
            .expect("fixture has one durable preparation result per attempt")
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
        unreachable!("the retry fixture dispatches its first attempt and rejects its second")
    }
}
