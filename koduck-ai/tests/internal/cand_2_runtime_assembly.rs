// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime-assembly harness for the runner's C-5 tool-call executor.

use crate::test_support::process_local_durable_claims;
use koduck_ai::adapters::tool::parse_input_schema;
use koduck_ai::application::{
    AttemptCommitResult, AttemptCommitter, AttemptStoreError, ExecutionAttemptInterruptionGuard,
    ExecutionAttemptLiveness, LeaseCheck, LeaseValidator, ModelToolCall, NewItem, ToolCallExecutor,
    ToolCallTurnContext, ToolConfigurationSnapshot, ToolExecutionOutcome, ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::execution::ExactActionBinding;
use koduck_ai::domain::tool::{CapabilityDescriptor, DescriptorState, Effect, PermissionProfile};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};
use koduck_ai::runtime::RuntimeState;
use koduck_ai::runtime::tool_executor::BoundaryToolCallExecutor;

/// Lease double for the assembly harness: these legs exercise Tool-call
/// servicing only and never observe a fenced generation, so the
/// current-generation answer is never load-bearing.
#[derive(Clone, Copy)]
struct UnusedInterruptionLease;

impl LeaseValidator for UnusedInterruptionLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Lease double that reports the bound generation as durably fenced, so the
/// servicing path must observe it through the injected validator instead of
/// trusting the synchronous servicing window (ADR-0003 TC-07).
#[derive(Clone, Copy)]
struct DurableFencedLease;

impl LeaseValidator for DurableFencedLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Fenced
    }
}

/// Counting committer double: always wins the conditional commit locally, so
/// the assembly harness observes the C-5 path without durable storage. The
/// commit counter is shared because the executor clones its committer for
/// each serviced call.
#[derive(Clone, Default)]
struct RecordingCommitter {
    commits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl RecordingCommitter {
    fn commits(&self) -> usize {
        self.commits.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl AttemptCommitter for RecordingCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, koduck_ai::application::AttemptCommitError> {
        self.commits
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(AttemptCommitResult::Won)
    }
}

impl ExecutionAttemptLiveness for RecordingCommitter {
    fn has_live_attempt(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        Ok(false)
    }
}

impl ExecutionAttemptInterruptionGuard for RecordingCommitter {
    fn begin_interruption(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<(), AttemptStoreError> {
        Ok(())
    }
}

/// Committer fixture for a D-7 owned by another process. The local runtime
/// catalog is intentionally empty, so the interruption path must not infer
/// that the durable Turn has no live execution work.
#[derive(Clone, Copy, Default)]
struct RemoteLiveCommitter;

impl AttemptCommitter for RemoteLiveCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, koduck_ai::application::AttemptCommitError> {
        unreachable!("a remote attempt cannot be terminalized through the local catalog")
    }
}

impl ExecutionAttemptLiveness for RemoteLiveCommitter {
    fn has_live_attempt(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        Ok(true)
    }
}

impl ExecutionAttemptInterruptionGuard for RemoteLiveCommitter {
    fn begin_interruption(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<(), AttemptStoreError> {
        Ok(())
    }
}

fn context() -> ToolCallTurnContext {
    ToolCallTurnContext {
        tenant_id: TenantId::new("tenant-a").expect("valid tenant"),
        thread_id: ThreadId::new(),
        turn_id: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
    }
}

fn trust() -> TrustContext {
    TrustContext::new(
        TenantId::new("tenant-a").expect("valid tenant"),
        "subject-a",
    )
    .expect("valid principal")
}

fn call(name: &str) -> ModelToolCall {
    ModelToolCall {
        name: name.to_owned(),
        arguments: "{}".to_owned(),
    }
}

/// Recording in-memory projection sink for assembly-harness assertions.
#[derive(Default)]
struct RecordingProjections {
    items: Vec<NewItem>,
}

impl koduck_ai::application::ToolProjectionSink for RecordingProjections {
    fn append(
        &mut self,
        projection: &koduck_ai::application::ToolProjection,
    ) -> Result<(), koduck_ai::application::ToolProjectionError> {
        self.items.extend(projection.d3_items());
        Ok(())
    }

    fn publish(&mut self, _projection: &koduck_ai::application::ToolProjection) {}
}

fn tool_result_of(items: &[NewItem]) -> (&Option<koduck_ai::domain::execution::AttemptId>, &str) {
    let Some(NewItem::ToolResult {
        attempt_id, code, ..
    }) = items
        .iter()
        .find(|item| matches!(item, NewItem::ToolResult { .. }))
    else {
        panic!("the serviced call records a tool result");
    };
    (attempt_id, code.as_deref().unwrap_or("succeeded"))
}

process_local_durable_claims!(RecordingCommitter);
process_local_durable_claims!(RemoteLiveCommitter);

#[test]
fn runtime_assembly_denies_every_tool_call_through_the_empty_inventory() {
    let mut executor = RuntimeState::assemble().tool_call_executor(
        RecordingCommitter::default(),
        UnusedInterruptionLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let mut projections = RecordingProjections::default();
    let result = executor
        .execute_tool_call(call("any.tool"), &context(), &trust(), &mut projections)
        .expect("the denial is a recorded outcome, not a turn failure");
    let items = &projections.items;

    // TC-02/TC-13: the unknown descriptor denies with zero D-6/D-7 and zero
    // dispatch, recorded as typed items rather than an empty inventory error.
    assert_eq!(items.len(), 2);
    assert!(matches!(
        items[0],
        NewItem::ToolCall { ref descriptor_id, .. } if descriptor_id == "any.tool"
    ));
    assert_eq!(tool_result_of(items), (&None, "descriptor_missing"));
    assert!(result.is_error);
    assert_eq!(result.content, "descriptor_missing");
}

#[test]
fn interruption_with_remote_live_attempt_requires_reconciliation() {
    let root = ToolExecutionRuntimeRoot::issue();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        ToolConfigurationSnapshot::empty(),
        RemoteLiveCommitter,
        UnusedInterruptionLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let result = executor.request_interrupt(&trust(), ThreadId::new(), TurnId::new());

    assert!(
        matches!(
            result,
            Err(koduck_ai::application::ToolCallError::Reconciliation(_))
        ),
        "a process-local NoLiveAttempt must not authorize an interrupted Turn terminal"
    );
}

#[test]
fn runtime_assembly_normalizes_an_invalid_tool_name_before_recording_denial() {
    let mut executor = RuntimeState::assemble().tool_call_executor(
        RecordingCommitter::default(),
        UnusedInterruptionLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );
    let mut projections = RecordingProjections::default();

    let result = executor
        .execute_tool_call(
            call("fixture\u{7}tool"),
            &context(),
            &trust(),
            &mut projections,
        )
        .expect("an invalid unconfigured name is still a typed denial");

    assert!(matches!(
        projections.items.first(),
        Some(NewItem::ToolCall { descriptor_id, .. }) if descriptor_id.is_empty()
    ));
    assert!(result.is_error);
    assert_eq!(result.content, "descriptor_missing");
}

#[test]
fn runtime_assembly_records_the_full_c5_path_for_a_configured_capability() {
    // A synthetic in-profile read-only capability proves the executor's
    // resolved path drives the real C-5 boundary: the disabled production
    // executor's typed unavailability is recorded with the canonical D-7
    // identity from the emitted D-3 projections.
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(
            CapabilityDescriptor::new(
                "fixture.read",
                "v1",
                Effect::ReadData,
                DescriptorState::Active,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid schema"),
            )
            .expect("unique descriptor"),
        )
        .expect("descriptor registers");
    snapshot
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
                .expect("valid entry")
                .build(),
        )
        .expect("profile registers");
    let root = ToolExecutionRuntimeRoot::issue();
    let committer = RecordingCommitter::default();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        snapshot,
        committer.clone(),
        UnusedInterruptionLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let mut projections = RecordingProjections::default();
    let result = executor
        .execute_tool_call(call("fixture.read"), &context(), &trust(), &mut projections)
        .expect("the configured capability reaches a recorded terminal");
    let items = &projections.items;

    assert!(items.len() >= 2);
    assert!(matches!(
        items[items.len() - 2],
        NewItem::ToolCall { ref descriptor_id, ref target, .. }
            if descriptor_id == "fixture.read" && target == "fixture-target"
    ));
    let (attempt_id, code) = tool_result_of(items);
    let _ = result;
    assert!(
        attempt_id.is_some(),
        "the recorded terminal carries its canonical D-7 identity"
    );
    assert_eq!(code, "executor_unavailable");
    // The executor-unavailable failure carries effect state `not_started`,
    // so TC-08 permits exactly one automatic pre-effect retry: the initial
    // attempt and its fresh retry each commit one terminal.
    assert_eq!(
        committer.commits(),
        2,
        "the resolved C-5 path commits one terminal per attempt through the injected committer"
    );
}

#[test]
fn dispatch_path_uses_the_injected_durable_lease_validator() {
    // A configured capability whose injected validator reports the bound
    // generation as durably fenced must fail closed before any D-7
    // allocation or dispatch: the runtime executor passes its injected C-6
    // lease validator into the C-5 boundary's dispatch path rather than a
    // process-local stub that always answers Current (ADR-0003 TC-07).
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(
            CapabilityDescriptor::new(
                "fixture.read",
                "v1",
                Effect::ReadData,
                DescriptorState::Active,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid schema"),
            )
            .expect("unique descriptor"),
        )
        .expect("descriptor registers");
    snapshot
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
                .expect("valid entry")
                .build(),
        )
        .expect("profile registers");
    let root = ToolExecutionRuntimeRoot::issue();
    let committer = RecordingCommitter::default();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        snapshot,
        committer.clone(),
        DurableFencedLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let mut projections = RecordingProjections::default();
    let result =
        executor.execute_tool_call(call("fixture.read"), &context(), &trust(), &mut projections);

    assert!(
        matches!(
            result,
            Err(koduck_ai::application::ToolCallError::Preparation(
                koduck_ai::application::ExecutionPreparationError::OwnerFenced
            ))
        ),
        "a fenced injected lease fails the dispatch path closed before allocation, found {result:?}"
    );
    assert_eq!(
        committer.commits(),
        0,
        "a fenced owner commits no terminal through the injected committer"
    );
}

#[test]
fn policy_denials_are_recorded_tool_results_not_turn_terminals() {
    // A registered but disabled descriptor resolves by name, then C-5's
    // default-deny policy rejects it: the typed denial returns as a recorded
    // tool result with zero D-6/D-7 and zero dispatch instead of owning the
    // Turn terminal (ADR-0003 TC-02).
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(
            CapabilityDescriptor::new(
                "fixture.disabled",
                "v1",
                Effect::ReadData,
                DescriptorState::Disabled,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid schema"),
            )
            .expect("unique descriptor"),
        )
        .expect("descriptor registers");
    snapshot
        .register_profile(
            PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.disabled", "v1", Effect::ReadData, "fixture-target")
                .expect("valid entry")
                .build(),
        )
        .expect("profile registers");
    let root = ToolExecutionRuntimeRoot::issue();
    let committer = RecordingCommitter::default();
    let mut executor = BoundaryToolCallExecutor::new(
        &root,
        snapshot,
        committer.clone(),
        UnusedInterruptionLease,
        koduck_ai::application::NoToolAudits,
        koduck_ai::application::NoCanonicalTurnTerminal,
    );

    let mut projections = RecordingProjections::default();
    let result = executor
        .execute_tool_call(
            call("fixture.disabled"),
            &context(),
            &trust(),
            &mut projections,
        )
        .expect("a typed denial is a recorded outcome, never a turn failure");

    assert_eq!(projections.items.len(), 2);
    let (attempt_id, code) = tool_result_of(&projections.items);
    assert_eq!(attempt_id, &None, "a denial allocates no D-7");
    assert_eq!(code, "descriptor_disabled");
    assert!(result.is_error);
    assert_eq!(result.content, "descriptor_disabled");
    assert_eq!(
        committer.commits(),
        0,
        "a policy denial commits no terminal"
    );
}
