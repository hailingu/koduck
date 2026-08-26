// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! CAND-3 correction Item schema, durable codec, and raw-replay structure
//! contract tests (ADR-0003 AC-1 and AC-2). Every fixture asserts against the
//! normative clauses CR-01 through CR-07 of
//! `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` and
//! the implementation copy in
//! `koduck-ai/docs/contracts/cand-3-correction-schema-v1.md`.

use koduck_ai::adapters::history::postgres::{DurableItemCodec, DurableItemColumns};
use koduck_ai::adapters::http::{HttpAdapter, HttpMethod, HttpRequest, ServiceError, TurnService};
use koduck_ai::application::{HistoryError, TurnCommand, TurnResult};
use koduck_ai::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};
use koduck_ai::domain::item_correction::{
    ItemCorrection, RawReplayStructureError, validate_raw_replay,
};
use koduck_ai::domain::{
    DomainValueError, Item, ItemId, ItemPayload, TenantId, TerminalOutcome, ThreadId,
    ToolEffectState, TrustContext, TurnId, TurnStatus, Usage,
};

/// AC-1: the typed correction representation and the durable codec implement
/// CR-01 and CR-05 while every existing Item kind round-trips unchanged and
/// every adjacent exhaustive-match site keeps the correction non-publishing
/// and non-integrating (CR-07).
#[test]
fn codec_and_compatibility() {
    every_existing_kind_round_trips_unchanged();
    valid_correction_round_trips_with_exact_content_and_target();
    blank_replacement_content_is_not_representable();
    malformed_correction_rows_fail_closed();
    correction_remains_non_publishing_on_the_wire();
}

/// AC-2: raw replay keeps every original and correction Item exactly once in
/// increasing sequence order (CR-04) and fails closed on every invalid
/// structure (CR-02, CR-03, CR-05) without hiding, substituting, or
/// reordering any Item.
#[test]
fn raw_replay() {
    ordered_mixed_history_validates();
    self_reference_fails_closed();
    absent_target_fails_closed();
    duplicate_direct_successor_fails_closed();
    non_increasing_and_duplicate_identity_fail_closed();
}

fn every_existing_kind_round_trips_unchanged() {
    for payload in existing_payload_fixtures() {
        let DurableItemColumns {
            item_type,
            payload: encoded,
            is_terminal,
            terminal_status,
            corrects_item_id,
        } = DurableItemCodec::encode(&payload);
        // CR-06: no existing representation changes; only a correction row
        // may carry a relationship identity (CR-02).
        assert_eq!(
            corrects_item_id, None,
            "{item_type} must not gain a relationship"
        );
        let decoded = DurableItemCodec::decode(item_type, &encoded, None);
        assert_eq!(
            decoded.as_ref(),
            Ok(&payload),
            "{item_type} must round-trip unchanged"
        );
        let expects_terminal = matches!(payload, ItemPayload::Terminal(_));
        assert_eq!(
            is_terminal, expects_terminal,
            "{item_type} terminal flag must stay exact"
        );
        if !expects_terminal {
            assert_eq!(terminal_status, None, "{item_type} has no terminal status");
        }
    }
}

fn valid_correction_round_trips_with_exact_content_and_target() {
    let target = ItemId::new();
    let content = "replacement \"quoted\" \\ é\n".to_owned();
    let correction =
        ItemCorrection::new(content.clone(), target).expect("valid correction content");
    let payload = ItemPayload::Correction(correction.clone());

    let DurableItemColumns {
        item_type,
        payload: encoded,
        is_terminal,
        terminal_status,
        corrects_item_id,
    } = DurableItemCodec::encode(&payload);
    assert_eq!(item_type, "correction");
    assert!(!is_terminal, "a correction is never a terminal Item");
    assert_eq!(terminal_status, None);
    assert_eq!(corrects_item_id, Some(target.as_uuid()));
    // The predecessor identity lives only in the durable relationship column;
    // the canonical payload JSON carries exactly the replacement content.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).expect("valid canonical JSON"),
        serde_json::json!({ "content": content }),
    );

    let decoded = DurableItemCodec::decode("correction", &encoded, Some(target.as_uuid()));
    assert_eq!(
        decoded,
        Ok(ItemPayload::Correction(correction)),
        "the correction must round-trip with exact content and target"
    );
}

fn blank_replacement_content_is_not_representable() {
    let target = ItemId::new();
    for blank in ["", "  \t\n"] {
        assert_eq!(
            ItemCorrection::new(blank, target).unwrap_err(),
            DomainValueError::Empty { field: "content" },
            "blank replacement content must not be representable"
        );
    }
}

fn malformed_correction_rows_fail_closed() {
    let target = ItemId::new().as_uuid();
    let valid = serde_json::json!({ "content": "replacement" }).to_string();
    for (label, item_type, payload, corrects) in [
        ("content member absent", "correction", "{}", None),
        (
            "content member is not a string",
            "correction",
            r#"{"content": 7}"#,
            None,
        ),
        ("content is empty", "correction", r#"{"content": ""}"#, None),
        (
            "content is blank",
            "correction",
            r#"{"content": "   "}"#,
            None,
        ),
        (
            "relationship column absent",
            "correction",
            valid.as_str(),
            None,
        ),
        ("payload is a JSON array", "correction", "[1,2]", None),
        ("payload is a JSON string", "correction", r#""text""#, None),
        ("payload is not JSON", "correction", "not json {", None),
        (
            "payload carries one extra member",
            "correction",
            r#"{"content":"x","unexpected":true}"#,
            Some(target),
        ),
        (
            "payload carries a null extra member",
            "correction",
            r#"{"content":"x","unexpected":null}"#,
            Some(target),
        ),
        (
            "payload duplicates the content member",
            "correction",
            r#"{"content":"first","content":"second"}"#,
            Some(target),
        ),
        (
            "unknown discriminator variant",
            "correction_v2",
            valid.as_str(),
            None,
        ),
        (
            "unknown discriminator case",
            "Correction",
            valid.as_str(),
            None,
        ),
        (
            "unknown discriminator alias",
            "corrects",
            valid.as_str(),
            None,
        ),
        ("unknown discriminator empty", "", valid.as_str(), None),
        (
            "relationship on user message",
            "user_message",
            r#"{"content": "x"}"#,
            Some(target),
        ),
    ] {
        assert_eq!(
            DurableItemCodec::decode(item_type, payload, corrects),
            Err(HistoryError::Unavailable),
            "malformed case '{label}' must fail closed"
        );
    }

    // No existing kind may carry a durable relationship identity (CR-02).
    for payload in existing_payload_fixtures() {
        let DurableItemColumns {
            item_type, payload, ..
        } = DurableItemCodec::encode(&payload);
        assert_eq!(
            DurableItemCodec::decode(item_type, &payload, Some(ItemId::new().as_uuid())),
            Err(HistoryError::Unavailable),
            "{item_type} must reject a correction relationship column"
        );
    }

    oversized_stored_content_decodes_exactly();
}

/// CR-05 and the resource-bounds matrix: the decoder allocates only the
/// owned payload, so an at-limit and an over-limit stored content decode
/// exactly instead of inventing a second replay-side byte bound.
fn oversized_stored_content_decodes_exactly() {
    for size in [1_048_576, 1_048_577] {
        let oversized = serde_json::json!({ "content": "x".repeat(size) }).to_string();
        let decoded =
            DurableItemCodec::decode("correction", &oversized, Some(ItemId::new().as_uuid()));
        match decoded {
            Ok(ItemPayload::Correction(correction)) => {
                assert_eq!(
                    correction.content().len(),
                    size,
                    "oversized content must decode exactly once"
                );
            }
            other => panic!("oversized content of {size} bytes must decode: {other:?}"),
        }
    }
}

fn correction_remains_non_publishing_on_the_wire() {
    let target = ItemId::new();
    let correction_item = Item::new(
        2,
        ItemPayload::Correction(
            ItemCorrection::new("replacement", target).expect("valid correction content"),
        ),
    );
    let delta = Item::new(
        3,
        ItemPayload::AgentMessageDelta {
            content: "visible".to_owned(),
        },
    );
    let terminal = Item::new(
        4,
        ItemPayload::Terminal(TerminalOutcome::Completed {
            usage: Usage::zero(),
        }),
    );
    let mut adapter = HttpAdapter::new(StubTurns {
        result: TurnResult {
            thread_id: ThreadId::new(),
            turn_id: TurnId::new(),
            status: TurnStatus::Completed,
            published: vec![correction_item.clone(), delta.clone()],
            replay: vec![correction_item, delta, terminal],
        },
    });

    let sync = adapter.handle(post("/api/v1/ai/chat", r#"{"input":"hello"}"#));
    assert_eq!(sync.status, 200);
    let document: serde_json::Value = serde_json::from_str(&sync.body).expect("valid sync JSON");
    let items = document["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the delta is a sync item: {items:?}");
    assert_eq!(items[0]["type"], "agent_message_delta");
    assert_eq!(items[0]["content"], "visible");
    assert!(
        !sync.body.contains(&target.as_uuid().to_string()),
        "no correction identity may leak onto the wire (CR-07)"
    );
    assert!(
        !sync.body.contains("correction"),
        "no correction event or item may leak onto the wire (CR-07)"
    );

    let sse = adapter.handle(post("/api/v1/ai/chat/stream", r#"{"input":"hello"}"#));
    assert_eq!(sse.status, 200);
    assert!(
        !sse.body.contains("correction"),
        "no correction SSE event may be published (CR-07)"
    );
}

fn ordered_mixed_history_validates() {
    let user = Item::new(
        1,
        ItemPayload::UserMessage {
            content: "original".to_owned(),
        },
    );
    let delta = Item::new(
        2,
        ItemPayload::AgentMessageDelta {
            content: "delta".to_owned(),
        },
    );
    let user_fix = Item::new(
        7,
        ItemPayload::Correction(
            ItemCorrection::new("fixed user text", user.item_id).expect("valid correction"),
        ),
    );
    let delta_fix = Item::new(
        8,
        ItemPayload::Correction(
            ItemCorrection::new("fixed delta", delta.item_id).expect("valid correction"),
        ),
    );
    let chain = Item::new(
        9,
        ItemPayload::Correction(
            ItemCorrection::new("fixed again", user_fix.item_id).expect("valid correction"),
        ),
    );
    // Non-contiguous sequences stay valid: CR-04 requires increasing order,
    // not adjacency, so this fixture also proves no contiguity assumption.
    let history = vec![user, delta, user_fix, delta_fix, chain];
    let snapshot = history.clone();
    assert_eq!(validate_raw_replay(&history), Ok(()));
    assert_eq!(
        history, snapshot,
        "raw replay validation must never mutate, drop, or reorder an Item"
    );

    assert_eq!(validate_raw_replay(&[]), Ok(()));
}

fn self_reference_fails_closed() {
    let mut self_correcting = Item::new(
        2,
        ItemPayload::Correction(
            ItemCorrection::new("self", ItemId::new()).expect("valid correction content"),
        ),
    );
    self_correcting.payload = ItemPayload::Correction(
        ItemCorrection::new("self", self_correcting.item_id).expect("valid correction content"),
    );
    let history = vec![
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "original".to_owned(),
            },
        ),
        self_correcting,
    ];
    assert_eq!(
        validate_raw_replay(&history),
        Err(RawReplayStructureError::SelfCorrection),
        "a correcting Item must not identify itself (CR-02)"
    );
}

fn absent_target_fails_closed() {
    let foreign = ItemId::new();
    let history = vec![
        Item::new(
            1,
            ItemPayload::UserMessage {
                content: "original".to_owned(),
            },
        ),
        Item::new(
            2,
            ItemPayload::Correction(
                ItemCorrection::new("cross-scope", foreign).expect("valid correction content"),
            ),
        ),
    ];
    assert_eq!(
        validate_raw_replay(&history),
        Err(RawReplayStructureError::UnknownCorrectionTarget),
        "a target outside the replayed Turn must fail closed (CR-02, CR-05)"
    );
}

fn duplicate_direct_successor_fails_closed() {
    let original = Item::new(
        1,
        ItemPayload::UserMessage {
            content: "original".to_owned(),
        },
    );
    let first = Item::new(
        2,
        ItemPayload::Correction(
            ItemCorrection::new("first", original.item_id).expect("valid correction content"),
        ),
    );
    let second = Item::new(
        3,
        ItemPayload::Correction(
            ItemCorrection::new("second", original.item_id).expect("valid correction content"),
        ),
    );
    assert_eq!(
        validate_raw_replay(&[original, first, second]),
        Err(RawReplayStructureError::DuplicateSuccessor),
        "one predecessor has at most one direct successor (CR-03)"
    );
}

fn non_increasing_and_duplicate_identity_fail_closed() {
    let one = Item::new(
        1,
        ItemPayload::UserMessage {
            content: "one".to_owned(),
        },
    );
    let two = Item::new(
        2,
        ItemPayload::UserMessage {
            content: "two".to_owned(),
        },
    );
    assert_eq!(
        validate_raw_replay(&[two.clone(), one.clone()]),
        Err(RawReplayStructureError::NonIncreasingSequence),
        "replay order must be strictly increasing (CR-04)"
    );
    let mut equal = one.clone();
    equal.sequence = 1;
    assert_eq!(
        validate_raw_replay(&[one.clone(), equal]),
        Err(RawReplayStructureError::NonIncreasingSequence),
        "a repeated sequence position is not increasing (CR-04)"
    );
    let mut duplicate_identity = two.clone();
    duplicate_identity.item_id = one.item_id;
    assert_eq!(
        validate_raw_replay(&[one, duplicate_identity]),
        Err(RawReplayStructureError::DuplicateItemIdentity),
        "every Item replays exactly once (CR-04)"
    );
}

/// Every CAND-1/CAND-2 payload shape that must keep its exact durable
/// representation (CR-01, CR-06).
fn existing_payload_fixtures() -> Vec<ItemPayload> {
    let attempt = AttemptId::new();
    let mut fixtures = vec![
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
            status: ApprovalStatus::Requested,
            decision: None,
            version: 1,
        },
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
    ];
    for (status, effect) in [
        (ExecutionStatus::TimedOut, ToolEffectState::Unknown),
        (ExecutionStatus::Cancelled, ToolEffectState::NotStarted),
    ] {
        fixtures.push(ItemPayload::ToolResult {
            attempt_id: Some(attempt),
            status,
            code: None,
            effect_state: Some(effect),
            output_bytes: 0,
            output_digest: None,
            version: Some(3),
        });
    }
    fixtures
}

struct StubTurns {
    result: TurnResult,
}

impl TurnService for StubTurns {
    fn execute(&mut self, _command: TurnCommand) -> Result<TurnResult, ServiceError> {
        Ok(self.result.clone())
    }

    fn interrupt(&mut self, _trust: &TrustContext, _turn_id: TurnId) -> Result<(), ServiceError> {
        Ok(())
    }
}

fn post(path: &str, body: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Post,
        path: path.to_owned(),
        content_type: Some("application/json".to_owned()),
        body: body.to_owned(),
        trust: Some(
            TrustContext::new(
                TenantId::new("tenant-a").expect("valid tenant"),
                "subject-a",
            )
            .expect("valid trust context"),
        ),
    }
}
