// ADR: koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md

//! Characterization of the outbound wire documents: every published item
//! variant is byte-identical across buffered and live SSE publication, and
//! every non-item document keeps its exact ordered shape (ADR-0002
//! AC-2/AC-3).

use crate::application::{TurnResult, TurnStreamEvent};
use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};
use crate::domain::{
    Item, ItemPayload, TerminalOutcome, ThreadId, ToolEffectState, TurnId, TurnStatus, Usage,
};

use super::{interrupt_body, problem_body, sse_body, stream_event_body, sync_body};

fn approval_item(status: ApprovalStatus, decision: Option<ApprovalDecision>) -> Item {
    Item::new(
        2,
        ItemPayload::ApprovalStatus {
            approval_id: ApprovalId::new(),
            attempt_id: AttemptId::new(),
            status,
            decision,
            version: 2,
        },
    )
}

#[test]
fn streamed_and_buffered_approval_projections_carry_the_same_decision_field() {
    for (status, decision, expected) in [
        (
            ApprovalStatus::Accepted,
            Some(ApprovalDecision::Accepted),
            "\"decision\":\"accepted\"",
        ),
        (
            ApprovalStatus::Declined,
            Some(ApprovalDecision::Declined),
            "\"decision\":\"declined\"",
        ),
        (
            ApprovalStatus::Cancelled,
            Some(ApprovalDecision::Cancelled),
            "\"decision\":\"cancelled\"",
        ),
        (ApprovalStatus::Requested, None, "\"decision\":null"),
        (ApprovalStatus::Expired, None, "\"decision\":null"),
    ] {
        let item = approval_item(status, decision);
        let streamed = stream_event_body(TurnStreamEvent::Item {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            item: item.clone(),
        });
        assert!(
            streamed.contains(expected),
            "the streamed projection carries {expected}: {streamed}"
        );
        let buffered = sse_body(&TurnResult {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            status: TurnStatus::Completed,
            published: vec![item],
            replay: Vec::new(),
        });
        assert!(
            buffered.contains(expected),
            "the buffered projection carries {expected}: {buffered}"
        );
    }
}

/// Asserts one published item serializes to the same exact SSE event
/// block through the live stream and the buffered body, and that the
/// block is the expected wire document (TW-02/TW-03).
fn assert_item_document_parity(
    thread_id: ThreadId,
    turn_id: TurnId,
    item: Item,
    status: TurnStatus,
    expected_event: &str,
    expected_data: &str,
) {
    let live = stream_event_body(TurnStreamEvent::Item {
        thread_id,
        turn_id,
        item: item.clone(),
    });
    let expected_block = format!("event: {expected_event}\ndata: {expected_data}");
    assert_eq!(
        live,
        format!("{expected_block}\n\n"),
        "the live document is the exact wire shape"
    );
    let buffered = sse_body(&TurnResult {
        thread_id,
        turn_id,
        status,
        published: vec![item],
        replay: Vec::new(),
    });
    let blocks = buffered
        .split("\n\n")
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        blocks.len(),
        2,
        "buffered SSE is turn.started plus the one item event: {buffered}"
    );
    assert_eq!(
        blocks[1], expected_block,
        "the buffered document is byte-identical to the live document"
    );
}

fn placeholder_document(
    template: &str,
    thread_id: ThreadId,
    turn_id: TurnId,
    item: &Item,
) -> String {
    template
        .replace("{{thread_id}}", &thread_id.as_uuid().to_string())
        .replace("{{turn_id}}", &turn_id.as_uuid().to_string())
        .replace("{{item_id}}", &item.item_id.as_uuid().to_string())
}

#[allow(clippy::too_many_lines)] // one exhaustive wire fixture table (ADR-0002 AC-2)
#[test]
fn buffered_and_live_item_documents_are_byte_identical() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();

    let item = Item::new(
        2,
        ItemPayload::AgentMessageDelta {
            content: "ctrl\u{0}\u{7}\u{1f}tab\tline\nquote\"slash\\snow ☃🎉".to_owned(),
        },
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"agent_message_delta","content":"ctrl\u0000\u0007\u001ftab\tline\nquote\"slash\\snow ☃🎉"}"#,
        thread_id,
        turn_id,
        &item,
    );
    let document: serde_json::Value =
        serde_json::from_str(&expected).expect("the agent document parses");
    assert_eq!(
        document["content"].as_str(),
        Some("ctrl\u{0}\u{7}\u{1f}tab\tline\nquote\"slash\\snow ☃🎉")
    );
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "item.created",
        &expected,
    );

    for (status, status_wire, decision, decision_wire) in [
        (ApprovalStatus::Requested, "requested", None, "null"),
        (
            ApprovalStatus::Accepted,
            "accepted",
            Some(ApprovalDecision::Accepted),
            "\"accepted\"",
        ),
        (
            ApprovalStatus::Declined,
            "declined",
            Some(ApprovalDecision::Declined),
            "\"declined\"",
        ),
        (
            ApprovalStatus::Cancelled,
            "cancelled",
            Some(ApprovalDecision::Cancelled),
            "\"cancelled\"",
        ),
        (ApprovalStatus::Expired, "expired", None, "null"),
    ] {
        let approval_id = ApprovalId::new();
        let attempt_id = AttemptId::new();
        let item = Item::new(
            2,
            ItemPayload::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version: 2,
            },
        );
        let expected = placeholder_document(
            r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"approval_status","approval_id":"{{approval_id}}","attempt_id":"{{attempt_id}}","status":"{{approval_status}}","decision":{{approval_decision}},"version":2}"#,
            thread_id,
            turn_id,
            &item,
        )
        .replace("{{approval_id}}", &approval_id.as_uuid().to_string())
        .replace("{{attempt_id}}", &attempt_id.as_uuid().to_string())
        .replace("{{approval_status}}", status_wire)
        .replace("{{approval_decision}}", decision_wire);
        let document: serde_json::Value =
            serde_json::from_str(&expected).expect("the approval document parses");
        if decision_wire == "null" {
            assert!(document["decision"].is_null());
        } else {
            assert_eq!(
                document["decision"].as_str(),
                Some(decision_wire.trim_matches('"'))
            );
        }
        assert_item_document_parity(
            thread_id,
            turn_id,
            item,
            TurnStatus::Completed,
            "item.created",
            &expected,
        );
    }

    let dispatch_attempt = AttemptId::new();
    let item = Item::new(
        2,
        ItemPayload::ToolCall {
            descriptor_id: "fs.read".to_owned(),
            descriptor_version: "3".to_owned(),
            target: "file://reports/q1.txt".to_owned(),
            attempt_id: Some(dispatch_attempt),
            status: Some(ExecutionStatus::Running),
            version: Some(4),
        },
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"tool_call","descriptor_id":"fs.read","descriptor_version":"3","target":"file://reports/q1.txt","attempt_id":"{{attempt_id}}","status":"running","version":4}"#,
        thread_id,
        turn_id,
        &item,
    )
    .replace("{{attempt_id}}", &dispatch_attempt.as_uuid().to_string());
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "item.created",
        &expected,
    );

    let item = Item::new(
        2,
        ItemPayload::ToolCall {
            descriptor_id: "net.fetch".to_owned(),
            descriptor_version: "1".to_owned(),
            target: "https://example.test".to_owned(),
            attempt_id: None,
            status: None,
            version: None,
        },
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"tool_call","descriptor_id":"net.fetch","descriptor_version":"1","target":"https://example.test","attempt_id":null,"status":null,"version":null}"#,
        thread_id,
        turn_id,
        &item,
    );
    let document: serde_json::Value =
        serde_json::from_str(&expected).expect("the denied tool call document parses");
    assert!(document["attempt_id"].is_null());
    assert!(document["status"].is_null());
    assert!(document["version"].is_null());
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "item.created",
        &expected,
    );

    let result_attempt = AttemptId::new();
    let item = Item::new(
        2,
        ItemPayload::ToolResult {
            attempt_id: Some(result_attempt),
            status: ExecutionStatus::Succeeded,
            code: None,
            effect_state: Some(ToolEffectState::Started),
            output_bytes: 4096,
            output_digest: Some("sha256:9f2c".to_owned()),
            version: Some(3),
        },
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"tool_result","attempt_id":"{{attempt_id}}","status":"succeeded","code":null,"effect_state":"started","output_bytes":4096,"output_digest":"sha256:9f2c","version":3}"#,
        thread_id,
        turn_id,
        &item,
    )
    .replace("{{attempt_id}}", &result_attempt.as_uuid().to_string());
    let document: serde_json::Value =
        serde_json::from_str(&expected).expect("the succeeded tool result document parses");
    assert_eq!(document["output_bytes"].as_u64(), Some(4096));
    assert!(document["code"].is_null());
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "item.created",
        &expected,
    );

    let item = Item::new(
        2,
        ItemPayload::ToolResult {
            attempt_id: None,
            status: ExecutionStatus::Failed,
            code: Some("tool_failed".to_owned()),
            effect_state: None,
            output_bytes: 0,
            output_digest: None,
            version: None,
        },
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"item_id":"{{item_id}}","type":"tool_result","attempt_id":null,"status":"failed","code":"tool_failed","effect_state":null,"output_bytes":0,"output_digest":null,"version":null}"#,
        thread_id,
        turn_id,
        &item,
    );
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "item.created",
        &expected,
    );

    let usage = Usage::new(3, 2).expect("valid usage");
    let item = Item::new(
        2,
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
    );
    let expected = placeholder_document(
        r#"{"thread_id":"{{thread_id}}","turn_id":"{{turn_id}}","sequence":2,"status":"completed","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}"#,
        thread_id,
        turn_id,
        &item,
    );
    assert_item_document_parity(
        thread_id,
        turn_id,
        item,
        TurnStatus::Completed,
        "turn.completed",
        &expected,
    );

    for (outcome, status, event, status_wire) in [
        (
            TerminalOutcome::Failed {
                code: "provider_unavailable".to_owned(),
            },
            TurnStatus::Failed,
            "turn.failed",
            "failed",
        ),
        (
            TerminalOutcome::Interrupted,
            TurnStatus::Interrupted,
            "turn.interrupted",
            "interrupted",
        ),
        (
            TerminalOutcome::Cancelled,
            TurnStatus::Cancelled,
            "turn.cancelled",
            "cancelled",
        ),
    ] {
        let item = Item::new(2, ItemPayload::Terminal(outcome));
        let expected = placeholder_document(
            &format!(
                r#"{{"thread_id":"{{{{thread_id}}}}","turn_id":"{{{{turn_id}}}}","sequence":2,"status":"{status_wire}"}}"#
            ),
            thread_id,
            turn_id,
            &item,
        );
        let document: serde_json::Value =
            serde_json::from_str(&expected).expect("the terminal document parses");
        assert!(document.get("usage").is_none());
        assert_item_document_parity(thread_id, turn_id, item, status, event, &expected);
    }
}

#[allow(clippy::too_many_lines)] // one exhaustive non-item wire fixture table (ADR-0002 AC-3)
#[test]
fn typed_non_item_documents_preserve_exact_wire_shapes() {
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let started = format!(
        r#"{{"thread_id":"{}","turn_id":"{}","sequence":1,"status":"started"}}"#,
        thread_id.as_uuid(),
        turn_id.as_uuid()
    );
    assert_eq!(
        stream_event_body(TurnStreamEvent::Started { thread_id, turn_id }),
        format!("event: turn.started\ndata: {started}\n\n")
    );
    assert_eq!(
        sse_body(&TurnResult {
            thread_id,
            turn_id,
            status: TurnStatus::Completed,
            published: Vec::new(),
            replay: Vec::new(),
        }),
        format!("event: turn.started\ndata: {started}\n\n")
    );

    let turn_id = TurnId::new();
    assert_eq!(
        interrupt_body(turn_id),
        format!(
            r#"{{"turn_id":"{}","status":"interrupt-requested"}}"#,
            turn_id.as_uuid()
        )
    );

    for (status, code, title) in [
        (400u16, "invalid-request", "Invalid request"),
        (503u16, "provider-unavailable", "Provider unavailable"),
    ] {
        let body = problem_body(status, code);
        let document: serde_json::Value =
            serde_json::from_str(&body).expect("the problem body parses");
        let correlation = document["correlation_id"]
            .as_str()
            .expect("the correlation id is a string");
        uuid::Uuid::parse_str(correlation).expect("the correlation id is a UUID");
        assert_eq!(
            body.replace(correlation, "{{correlation_id}}"),
            format!(
                r#"{{"type":"about:blank","title":"{title}","status":{status},"code":"{code}","correlation_id":"{{{{correlation_id}}}}"}}"#
            )
        );
    }

    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let usage = Usage::new(3, 2).expect("valid usage");
    let replay = vec![
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "hello".to_owned(),
            },
        ),
        Item::new(
            2,
            ItemPayload::AgentMessageDelta {
                content: "A".to_owned(),
            },
        ),
        Item::new(
            3,
            ItemPayload::AgentMessageDelta {
                content: "B".to_owned(),
            },
        ),
        Item::new(4, ItemPayload::Usage(usage)),
        Item::new(
            5,
            ItemPayload::Terminal(TerminalOutcome::Completed { usage }),
        ),
    ];
    let delta_two = replay[1].item_id.as_uuid().to_string();
    let delta_three = replay[2].item_id.as_uuid().to_string();
    assert_eq!(
        sync_body(&TurnResult {
            thread_id,
            turn_id,
            status: TurnStatus::Completed,
            published: Vec::new(),
            replay,
        }),
        format!(
            r#"{{"thread_id":"{}","turn_id":"{}","status":"completed","items":[{{"item_id":"{delta_two}","sequence":2,"type":"agent_message_delta","content":"A"}},{{"item_id":"{delta_three}","sequence":3,"type":"agent_message_delta","content":"B"}}],"usage":{{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}}"#,
            thread_id.as_uuid(),
            turn_id.as_uuid()
        )
    );

    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    assert_eq!(
        sync_body(&TurnResult {
            thread_id,
            turn_id,
            status: TurnStatus::Completed,
            published: Vec::new(),
            replay: vec![Item::new(
                1,
                ItemPayload::UserMessage {
                    content: "hello".to_owned(),
                },
            )],
        }),
        format!(
            r#"{{"thread_id":"{}","turn_id":"{}","status":"completed","items":[],"usage":{{"input_tokens":0,"output_tokens":0,"total_tokens":0}}}}"#,
            thread_id.as_uuid(),
            turn_id.as_uuid()
        )
    );

    let content: String = ('\u{0}'..='\u{1f}').collect();
    let thread_id = ThreadId::new();
    let turn_id = TurnId::new();
    let item = Item::new(
        2,
        ItemPayload::AgentMessageDelta {
            content: content.clone(),
        },
    );
    let live = stream_event_body(TurnStreamEvent::Item {
        thread_id,
        turn_id,
        item: item.clone(),
    });
    let data = live
        .split("data: ")
        .nth(1)
        .expect("the live document carries data")
        .trim_end();
    let document: serde_json::Value =
        serde_json::from_str(data).expect("the live data document parses");
    assert_eq!(document["content"].as_str(), Some(content.as_str()));
    let body = sync_body(&TurnResult {
        thread_id,
        turn_id,
        status: TurnStatus::Completed,
        published: Vec::new(),
        replay: vec![item],
    });
    let document: serde_json::Value =
        serde_json::from_str(&body).expect("the synchronous body parses");
    assert_eq!(
        document["items"][0]["content"].as_str(),
        Some(content.as_str())
    );
}
