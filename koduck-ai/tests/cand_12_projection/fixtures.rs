// ADR: koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md

//! Shared Item and scope fixtures for the CAND-12 projection acceptance
//! tests. These helpers build canonical domain values only; every assertion
//! lives in the named acceptance tests.

use koduck_ai::application::{ProjectionScope, ScopedProjectionItem};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};
use koduck_ai::domain::item_correction::ItemCorrection;
use koduck_ai::domain::{
    Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId, ToolEffectState, TurnId, Usage,
};

/// Builds one expected projection scope from already validated components.
pub(crate) fn scope_fixture(tenant: &str, subject: &str) -> ProjectionScope {
    ProjectionScope::new(
        TenantId::new(tenant).expect("valid tenant"),
        subject,
        ThreadId::new(),
        TurnId::new(),
    )
    .expect("valid scope")
}

/// Builds one user message Item at the given Turn-local sequence.
pub(crate) fn user_item(sequence: u64, content: &str) -> Item {
    Item::new(
        sequence,
        ItemPayload::UserMessage {
            content: content.to_owned(),
        },
    )
}

/// Builds one agent message delta Item at the given Turn-local sequence.
pub(crate) fn delta_item(sequence: u64, content: &str) -> Item {
    Item::new(
        sequence,
        ItemPayload::AgentMessageDelta {
            content: content.to_owned(),
        },
    )
}

/// Builds one correction Item at the given Turn-local sequence.
pub(crate) fn correction_item(sequence: u64, target: ItemId, content: &str) -> Item {
    Item::new(sequence, correction_payload(target, content))
}

/// Builds one typed correction payload.
pub(crate) fn correction_payload(target: ItemId, content: &str) -> ItemPayload {
    ItemPayload::Correction(ItemCorrection::new(content, target).expect("valid correction content"))
}

/// Returns the replacement content of one correction Item fixture.
pub(crate) fn correction_content(item: &Item) -> &str {
    match &item.payload {
        ItemPayload::Correction(correction) => correction.content(),
        _ => panic!("fixture must be a correction item"),
    }
}

/// Builds one usage Item at the given Turn-local sequence.
pub(crate) fn usage_item(sequence: u64) -> Item {
    Item::new(
        sequence,
        ItemPayload::Usage(Usage::new(3, 5).expect("valid usage")),
    )
}

/// Builds one approval status view Item at the given Turn-local sequence.
pub(crate) fn approval_item(sequence: u64) -> Item {
    Item::new(
        sequence,
        ItemPayload::ApprovalStatus {
            approval_id: ApprovalId::new(),
            attempt_id: AttemptId::new(),
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        },
    )
}

/// Builds one Tool call view Item at the given Turn-local sequence.
pub(crate) fn tool_call_item(sequence: u64) -> Item {
    Item::new(
        sequence,
        ItemPayload::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id: Some(AttemptId::new()),
            status: Some(ExecutionStatus::Running),
            version: Some(2),
        },
    )
}

/// Builds one Tool result view Item at the given Turn-local sequence.
pub(crate) fn tool_result_item(sequence: u64) -> Item {
    Item::new(
        sequence,
        ItemPayload::ToolResult {
            attempt_id: Some(AttemptId::new()),
            status: ExecutionStatus::Succeeded,
            code: None,
            effect_state: Some(ToolEffectState::Started),
            output_bytes: 3,
            output_digest: Some("a".repeat(64)),
            version: Some(3),
        },
    )
}

/// Builds one terminal Item at the given Turn-local sequence.
pub(crate) fn terminal_item(sequence: u64, outcome: TerminalOutcome) -> Item {
    Item::new(sequence, ItemPayload::Terminal(outcome))
}

/// Wraps every borrowed Item with one shared expected scope.
pub(crate) fn scoped_entries<'item, 'scope>(
    items: &'item [Item],
    scope: &'scope ProjectionScope,
) -> Vec<ScopedProjectionItem<'item, 'scope>> {
    items
        .iter()
        .map(|item| ScopedProjectionItem::new(item, scope))
        .collect()
}

/// Every existing non-correction `ItemPayload` variant, mirroring the CAND-3
/// durable fixtures.
pub(crate) fn non_correction_payload_fixtures() -> Vec<ItemPayload> {
    let attempt = AttemptId::new();
    vec![
        ItemPayload::UserMessage {
            content: "user \"quoted\" é".to_owned(),
        },
        ItemPayload::AgentMessageDelta {
            content: "delta \n\u{0001}".to_owned(),
        },
        ItemPayload::Usage(Usage::new(3, 5).expect("valid usage")),
        ItemPayload::Terminal(TerminalOutcome::Completed {
            usage: Usage::new(7, 11).expect("valid usage"),
        }),
        ItemPayload::Terminal(TerminalOutcome::Failed {
            code: "provider_failed".to_owned(),
        }),
        ItemPayload::Terminal(TerminalOutcome::Interrupted),
        ItemPayload::Terminal(TerminalOutcome::Cancelled),
        ItemPayload::ApprovalStatus {
            approval_id: ApprovalId::new(),
            attempt_id: AttemptId::new(),
            status: ApprovalStatus::Declined,
            decision: Some(ApprovalDecision::Declined),
            version: 2,
        },
        ItemPayload::ToolCall {
            descriptor_id: "fixture.tool".to_owned(),
            descriptor_version: "v1".to_owned(),
            target: "fixture-target".to_owned(),
            attempt_id: Some(attempt),
            status: Some(ExecutionStatus::Running),
            version: Some(2),
        },
        ItemPayload::ToolCall {
            descriptor_id: String::new(),
            descriptor_version: String::new(),
            target: String::new(),
            attempt_id: None,
            status: None,
            version: None,
        },
        ItemPayload::ToolResult {
            attempt_id: Some(attempt),
            status: ExecutionStatus::Failed,
            code: Some("attempt_limit".to_owned()),
            effect_state: Some(ToolEffectState::Started),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        },
        ItemPayload::ToolResult {
            attempt_id: None,
            status: ExecutionStatus::Failed,
            code: Some("descriptor_missing".to_owned()),
            effect_state: None,
            output_bytes: 0,
            output_digest: None,
            version: None,
        },
        ItemPayload::ToolResult {
            attempt_id: Some(attempt),
            status: ExecutionStatus::Succeeded,
            code: None,
            effect_state: Some(ToolEffectState::Started),
            output_bytes: 3,
            output_digest: Some("a".repeat(64)),
            version: Some(3),
        },
        ItemPayload::ToolResult {
            attempt_id: Some(attempt),
            status: ExecutionStatus::TimedOut,
            code: None,
            effect_state: Some(ToolEffectState::Unknown),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        },
        ItemPayload::ToolResult {
            attempt_id: Some(attempt),
            status: ExecutionStatus::Cancelled,
            code: None,
            effect_state: Some(ToolEffectState::NotStarted),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        },
    ]
}
