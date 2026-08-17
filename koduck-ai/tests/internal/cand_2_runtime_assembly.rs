// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime-assembly harness for the runner's C-5 tool-call executor.

use koduck_ai::adapters::tool::parse_input_schema;
use koduck_ai::application::{
    ModelToolCall, NewItem, ToolCallExecutor, ToolCallTurnContext, ToolConfigurationSnapshot,
    ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::tool::{CapabilityDescriptor, DescriptorState, Effect, PermissionProfile};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};
use koduck_ai::runtime::RuntimeState;
use koduck_ai::runtime::tool_executor::BoundaryToolCallExecutor;

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

#[test]
fn runtime_assembly_denies_every_tool_call_through_the_empty_inventory() {
    let mut executor = RuntimeState::assemble().tool_call_executor();

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
fn runtime_assembly_normalizes_an_invalid_tool_name_before_recording_denial() {
    let mut executor = RuntimeState::assemble().tool_call_executor();
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
    let mut executor = BoundaryToolCallExecutor::new(&root, snapshot);

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
    let mut executor = BoundaryToolCallExecutor::new(&root, snapshot);

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
}
