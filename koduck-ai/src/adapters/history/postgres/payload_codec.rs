// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `PostgreSQL` payload encoding and durable-row decoding for canonical Items.

use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::application::HistoryError;
use crate::domain::{Item, ItemPayload, TerminalOutcome, Usage};

mod item_correction;
mod tool_projections;

/// The durable `turn_items` column tuple of one encoded canonical Item.
#[derive(Clone, Debug)]
pub struct DurableItemColumns {
    /// Durable `item_type` discriminator.
    pub item_type: &'static str,
    /// Canonical serialized `payload` JSON text.
    pub payload: String,
    /// Whether the encoded Item is the Turn terminal.
    pub is_terminal: bool,
    /// The durable terminal status name, when the Item is terminal.
    pub terminal_status: Option<&'static str>,
    /// The durable correction relationship target, when the Item is a
    /// correction (ADR-0003 CR-02).
    pub corrects_item_id: Option<Uuid>,
}

/// The durable canonical-Item codec contract of the C-6 `PostgreSQL` history
/// adapter (ADR-0003 CR-01/CR-05).
///
/// Public so the durable representation stays deterministically verifiable
/// without a live database. Correction admission (CAND-11) and effective
/// projection (CAND-12) are owned by later records and MUST NOT grow this
/// contract into write behavior.
#[derive(Clone, Copy, Debug)]
pub struct DurableItemCodec;

impl DurableItemCodec {
    /// Encodes one owned payload into its durable column tuple.
    #[must_use]
    pub fn encode(payload: &ItemPayload) -> DurableItemColumns {
        let (item_type, payload_text, is_terminal, terminal_status, corrects_item_id) =
            encode_payload(payload);
        DurableItemColumns {
            item_type,
            payload: payload_text,
            is_terminal,
            terminal_status,
            corrects_item_id,
        }
    }

    /// Decodes one durable row into the owned payload, failing closed on
    /// every malformed shape (CR-05).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::Unavailable`] when the discriminator is
    /// unknown, the payload text is not the declared structure, or the
    /// relationship column disagrees with the discriminator.
    pub fn decode(
        item_type: &str,
        payload: &str,
        corrects_item_id: Option<Uuid>,
    ) -> Result<ItemPayload, HistoryError> {
        let payload: Value = parse_payload(item_type, payload)?;
        decode_payload(item_type, &payload, corrects_item_id)
    }
}

/// Parses one durable payload document, failing closed first for malformed
/// JSON and — for a correction row — for any duplicated object member:
/// `serde_json::Value` collapses duplicates, so the raw text must pass the
/// strict recursive validator before the last value can masquerade as the
/// single canonical member (ADR-0003 CR-05).
fn parse_payload(item_type: &str, payload: &str) -> Result<Value, HistoryError> {
    if item_type == item_correction::DISCRIMINATOR {
        crate::adapters::strict_json::ensure_unique_members(payload)
            .map_err(|_| HistoryError::Unavailable)?;
    }
    serde_json::from_str(payload).map_err(|_| HistoryError::Unavailable)
}

/// Encodes one owned payload into its `PostgreSQL` discriminator and JSON text.
// One exhaustive durable payload discriminator table; splitting it would
// separate a payload from its durable translation (ADR-0003 CR-01).
#[allow(clippy::too_many_lines)]
pub(super) fn encode_payload(
    payload: &ItemPayload,
) -> (
    &'static str,
    String,
    bool,
    Option<&'static str>,
    Option<Uuid>,
) {
    let (item_type, payload, is_terminal, terminal_status, corrects_item_id) = match payload {
        ItemPayload::UserMessage { content } => (
            "user_message",
            json!({ "content": content }),
            false,
            None,
            None,
        ),
        ItemPayload::AgentMessageDelta { content } => (
            "agent_message_delta",
            json!({ "content": content }),
            false,
            None,
            None,
        ),
        ItemPayload::Usage(usage) => ("usage", usage_json(*usage), false, None, None),
        ItemPayload::Terminal(TerminalOutcome::Completed { usage }) => (
            "completed",
            usage_json(*usage),
            true,
            Some("completed"),
            None,
        ),
        ItemPayload::Terminal(TerminalOutcome::Failed { code }) => (
            "failed",
            json!({ "code": code }),
            true,
            Some("failed"),
            None,
        ),
        ItemPayload::Terminal(TerminalOutcome::Interrupted) => {
            ("interrupted", json!({}), true, Some("interrupted"), None)
        }
        ItemPayload::Terminal(TerminalOutcome::Cancelled) => {
            ("cancelled", json!({}), true, Some("cancelled"), None)
        }
        ItemPayload::ApprovalStatus {
            approval_id,
            attempt_id,
            status,
            decision,
            version,
        } => (
            "approval_status",
            json!({
                "approval_id": approval_id.as_uuid().to_string(),
                "attempt_id": attempt_id.as_uuid().to_string(),
                "status": status.as_str(),
                "decision": decision.map(|decision| decision.as_str().to_owned()),
                "version": version,
            }),
            false,
            None,
            None,
        ),
        ItemPayload::ToolCall {
            descriptor_id,
            descriptor_version,
            target,
            attempt_id,
            status,
            version,
        } => (
            "tool_call",
            json!({
                "descriptor_id": descriptor_id,
                "descriptor_version": descriptor_version,
                "target": target,
                "attempt_id": attempt_id.map(|id| id.as_uuid().to_string()),
                "status": status.map(crate::domain::execution::ExecutionStatus::as_str),
                "version": version,
            }),
            false,
            None,
            None,
        ),
        ItemPayload::ToolResult {
            attempt_id,
            status,
            code,
            effect_state,
            output_bytes,
            output_digest,
            version,
        } => (
            "tool_result",
            json!({
                "attempt_id": attempt_id.map(|id| id.as_uuid().to_string()),
                "status": status.as_str(),
                "code": code,
                "effect_state": effect_state.map(effect_state_name),
                "output_bytes": output_bytes,
                "output_digest": output_digest,
                "version": version,
            }),
            false,
            None,
            None,
        ),
        ItemPayload::Correction(correction) => (
            item_correction::DISCRIMINATOR,
            item_correction::encode(correction),
            false,
            None,
            Some(correction.corrects_item_id().as_uuid()),
        ),
    };
    (
        item_type,
        payload.to_string(),
        is_terminal,
        terminal_status,
        corrects_item_id,
    )
}

/// Decodes one `PostgreSQL` row into the owned canonical Item representation.
pub(super) fn row_to_item(row: &PgRow) -> Result<Item, HistoryError> {
    let item_id: Uuid = row.try_get("item_id").map_err(unavailable)?;
    let sequence: i64 = row.try_get("sequence").map_err(unavailable)?;
    let item_type: String = row.try_get("item_type").map_err(unavailable)?;
    let payload: String = row.try_get("payload").map_err(unavailable)?;
    let corrects_item_id: Option<Uuid> = row.try_get("corrects_item_id").map_err(unavailable)?;
    let payload: Value = parse_payload(&item_type, &payload)?;
    let payload = decode_payload(&item_type, &payload, corrects_item_id)?;
    Ok(Item {
        item_id: crate::domain::ItemId::from_uuid(item_id),
        sequence: u64::try_from(sequence).map_err(|_| HistoryError::Unavailable)?,
        payload,
    })
}

fn usage_json(usage: Usage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.total_tokens,
    })
}

/// Decodes non-projection payloads after the projection decoder validates D-3.
fn decode_payload(
    item_type: &str,
    payload: &Value,
    corrects_item_id: Option<Uuid>,
) -> Result<ItemPayload, HistoryError> {
    // A durable correction relationship is legal only on a correction row:
    // any other shape is malformed externally inserted data and fails closed
    // (ADR-0003 CR-05).
    if corrects_item_id.is_some() && item_type != item_correction::DISCRIMINATOR {
        return Err(HistoryError::Unavailable);
    }
    if let Some(projection) = tool_projections::decode(item_type, payload)? {
        return Ok(projection);
    }
    let text = || {
        payload
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(HistoryError::Unavailable)
    };
    match item_type {
        "user_message" => Ok(ItemPayload::UserMessage { content: text()? }),
        "agent_message_delta" => Ok(ItemPayload::AgentMessageDelta { content: text()? }),
        "usage" => Ok(ItemPayload::Usage(decode_usage(payload)?)),
        "completed" => Ok(ItemPayload::Terminal(TerminalOutcome::Completed {
            usage: decode_usage(payload)?,
        })),
        "failed" => Ok(ItemPayload::Terminal(TerminalOutcome::Failed {
            code: payload
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(HistoryError::Unavailable)?,
        })),
        "interrupted" => Ok(ItemPayload::Terminal(TerminalOutcome::Interrupted)),
        "cancelled" => Ok(ItemPayload::Terminal(TerminalOutcome::Cancelled)),
        item_correction::DISCRIMINATOR => Ok(ItemPayload::Correction(item_correction::decode(
            payload,
            corrects_item_id,
        )?)),
        _ => Err(HistoryError::Unavailable),
    }
}

pub(super) fn field(payload: &Value, name: &str) -> Result<String, HistoryError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(HistoryError::Unavailable)
}

pub(super) fn parse_status(
    value: &str,
) -> Result<crate::domain::execution::ExecutionStatus, HistoryError> {
    use crate::domain::execution::ExecutionStatus;
    match value {
        "prepared" => Ok(ExecutionStatus::Prepared),
        "running" => Ok(ExecutionStatus::Running),
        "succeeded" => Ok(ExecutionStatus::Succeeded),
        "failed" => Ok(ExecutionStatus::Failed),
        "timed_out" => Ok(ExecutionStatus::TimedOut),
        "cancelled" => Ok(ExecutionStatus::Cancelled),
        _ => Err(HistoryError::Unavailable),
    }
}

fn decode_usage(payload: &Value) -> Result<Usage, HistoryError> {
    let input = payload
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let output = payload
        .get("output_tokens")
        .and_then(Value::as_u64)
        .ok_or(HistoryError::Unavailable)?;
    let usage = Usage::new(input, output).map_err(|_| HistoryError::Unavailable)?;
    (payload.get("total_tokens").and_then(Value::as_u64) == Some(usage.total_tokens))
        .then_some(usage)
        .ok_or(HistoryError::Unavailable)
}

fn unavailable(_error: sqlx::Error) -> HistoryError {
    HistoryError::Unavailable
}

pub(super) fn parse_approval_status(
    value: &str,
) -> Result<crate::domain::execution::ApprovalStatus, HistoryError> {
    use crate::domain::execution::ApprovalStatus;
    match value {
        "requested" => Ok(ApprovalStatus::Requested),
        "accepted" => Ok(ApprovalStatus::Accepted),
        "declined" => Ok(ApprovalStatus::Declined),
        "cancelled" => Ok(ApprovalStatus::Cancelled),
        "expired" => Ok(ApprovalStatus::Expired),
        _ => Err(HistoryError::Unavailable),
    }
}

pub(super) fn parse_approval_decision(
    value: &str,
) -> Result<crate::domain::execution::ApprovalDecision, HistoryError> {
    use crate::domain::execution::ApprovalDecision;
    match value {
        "accepted" => Ok(ApprovalDecision::Accepted),
        "declined" => Ok(ApprovalDecision::Declined),
        "cancelled" => Ok(ApprovalDecision::Cancelled),
        _ => Err(HistoryError::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::encode_payload;
    use crate::application::{AppendPolicy, NewItem};
    use crate::domain::execution::{
        ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
    };
    use crate::domain::{TerminalOutcome, Usage};

    /// The unpublished-buffer preflight accounting MUST equal the canonical
    /// encoded size for every payload shape; an undercount lets a near-limit
    /// batch pass preflight while its `PostgreSQL` JSON exceeds 1 MiB
    /// (ADR-0001 CAND-1 exact serialized-payload limit).
    #[test]
    fn preflight_accounting_matches_the_canonical_payload_encoding() {
        let mut items = vec![
            NewItem::AgentMessageDelta {
                content: "delta \"quoted\" \\ é\n\u{0001}".to_owned(),
            },
            NewItem::Usage(Usage::new(3, 5).expect("valid usage")),
            NewItem::Terminal(TerminalOutcome::Completed {
                usage: Usage::new(7, 11).expect("valid usage"),
            }),
            NewItem::Terminal(TerminalOutcome::Failed {
                code: "provider_failed \"x\"".to_owned(),
            }),
            NewItem::Terminal(TerminalOutcome::Interrupted),
            NewItem::Terminal(TerminalOutcome::Cancelled),
            NewItem::ToolCall {
                descriptor_id: "descriptor \"1\"".to_owned(),
                descriptor_version: "v1\n".to_owned(),
                target: "fixture-target é".to_owned(),
                attempt_id: None,
                status: None,
                version: None,
            },
        ];
        for status in [
            ApprovalStatus::Requested,
            ApprovalStatus::Accepted,
            ApprovalStatus::Declined,
            ApprovalStatus::Cancelled,
            ApprovalStatus::Expired,
        ] {
            for decision in [
                None,
                Some(ApprovalDecision::Accepted),
                Some(ApprovalDecision::Declined),
                Some(ApprovalDecision::Cancelled),
            ] {
                items.push(NewItem::ApprovalStatus {
                    approval_id: ApprovalId::new(),
                    attempt_id: AttemptId::new(),
                    status,
                    decision,
                    version: 42,
                });
            }
        }
        for status in [
            ExecutionStatus::Succeeded,
            ExecutionStatus::Failed,
            ExecutionStatus::TimedOut,
            ExecutionStatus::Cancelled,
        ] {
            for canonical in [false, true] {
                let attempt_id = canonical.then(AttemptId::new);
                // Only a failure carries the stable code; success, timeout,
                // and cancellation carry none, mirroring the canonical
                // projection shape.
                let code = (status == ExecutionStatus::Failed).then(|| "typed \"code\"".to_owned());
                let output_digest =
                    (canonical && status == ExecutionStatus::Succeeded).then(|| "a".repeat(64));
                items.push(NewItem::ToolResult {
                    attempt_id,
                    status,
                    code,
                    effect_state: canonical.then_some(crate::domain::ToolEffectState::Started),
                    output_bytes: 1_048_576,
                    output_digest,
                    version: canonical.then_some(3),
                });
            }
        }
        for item in items {
            let accounted = AppendPolicy::cand_1()
                .accumulate_payload_bytes(0, &item)
                .expect("one item fits the buffer");
            let canonical = encode_payload(&item.clone().into_payload()).1.len();
            assert_eq!(
                accounted, canonical,
                "preflight accounting must match the canonical encoding for {item:?}"
            );
        }
    }
}

/// Decodes a canonical optional UUID field: the member must be present, an
/// explicit JSON `null` means absence, and any other value must be a valid
/// UUID string — a corrupt canonical identity fails decoding instead of being
/// silently reinterpreted (ADR-0003 TC-06).
pub(super) fn required_optional_uuid(
    payload: &Value,
    name: &str,
) -> Result<Option<crate::domain::execution::AttemptId>, HistoryError> {
    match payload.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => uuid::Uuid::parse_str(id)
            .map(crate::domain::execution::AttemptId::from_uuid)
            .map(Some)
            .map_err(|_| HistoryError::Unavailable),
        _ => Err(HistoryError::Unavailable),
    }
}

/// Decodes a canonical optional text field under the same strict contract.
pub(super) fn required_optional_text(
    payload: &Value,
    name: &str,
) -> Result<Option<String>, HistoryError> {
    match payload.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(HistoryError::Unavailable),
    }
}

#[cfg(test)]
mod strict_tool_result_tests {
    use super::*;

    #[test]
    fn canonical_timeout_and_cancellation_results_replay_without_a_code() {
        // `outcome_failure_code` emits `None` for canonical TimedOut and
        // Cancelled terminals, so their `code: null` encoding must replay;
        // only a Failed terminal carries the stable failure code (ADR-0003
        // TC-06).
        for (status, effect_state) in [
            ("timed_out", "unknown"),
            ("cancelled", "not_started"),
            ("cancelled", "started"),
        ] {
            let encoded = serde_json::json!({
                "attempt_id": "00000000-0000-0000-0000-000000000001",
                "status": status,
                "code": null,
                "effect_state": effect_state,
                "output_bytes": 0,
                "output_digest": null,
                "version": 3,
            });
            let decoded = decode_payload("tool_result", &encoded, None);
            assert!(
                matches!(decoded, Ok(ItemPayload::ToolResult { code: None, .. })),
                "canonical {status} terminal must replay with code null: {decoded:?}"
            );
        }
        // A Failed terminal still requires its code, and a success, timeout,
        // or cancellation must not carry one.
        for corrupt in [
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "timed_out", "code": "timed_out", "effect_state": "unknown", "output_bytes": 0, "output_digest": null, "version": 3}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "cancelled", "code": "cancelled", "effect_state": "not_started", "output_bytes": 0, "output_digest": null, "version": 3}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "cancelled", "code": null, "effect_state": "unknown", "output_bytes": 0, "output_digest": null, "version": 3}),
        ] {
            assert_eq!(
                decode_payload("tool_result", &corrupt, None),
                Err(HistoryError::Unavailable),
                "a non-failed terminal carrying a code is not canonical: {corrupt}"
            );
        }
    }

    #[test]
    fn approval_status_replay_validates_the_canonical_tuple() {
        // Every canonical D-6 tuple round-trips: the state machine creates
        // `requested` at version 1 and performs exactly one terminal
        // transition to version 2; no decision while requested or expired,
        // or when an authenticated interruption owns cancellation; every
        // other terminal status carries its matching decision (migration
        // 0008_cand_2_interruption_approval_cancellation, ADR-0003 TC-06).
        for (status, decision, version) in [
            ("requested", None, 1),
            ("expired", None, 2),
            ("accepted", Some("accepted"), 2),
            ("declined", Some("declined"), 2),
            ("cancelled", Some("cancelled"), 2),
            ("cancelled", None, 2),
        ] {
            let encoded = serde_json::json!({
                "approval_id": "00000000-0000-0000-0000-000000000001",
                "attempt_id": "00000000-0000-0000-0000-000000000002",
                "status": status,
                "decision": decision,
                "version": version,
            });
            let decoded = decode_payload("approval_status", &encoded, None);
            assert!(
                matches!(decoded, Ok(ItemPayload::ApprovalStatus { .. })),
                "canonical approval tuple must replay: {decoded:?}"
            );
        }
        for corrupt in [
            // Version zero is not a canonical D-6 record version.
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "requested", "decision": null, "version": 0}),
            // Canonical versions are exact: `requested` is version 1 and the
            // single terminal transition is version 2.
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "requested", "decision": null, "version": 2}),
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "accepted", "decision": "accepted", "version": 1}),
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "accepted", "decision": "accepted", "version": 3}),
            // A requested or expired approval carries no decision.
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "requested", "decision": "accepted", "version": 1}),
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "expired", "decision": "cancelled", "version": 2}),
            // A terminal status pairs with exactly its matching decision.
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "accepted", "decision": "declined", "version": 2}),
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "declined", "decision": null, "version": 2}),
            serde_json::json!({"approval_id": "00000000-0000-0000-0000-000000000001", "status": "cancelled", "decision": "accepted", "version": 2}),
        ] {
            assert_eq!(
                decode_payload("approval_status", &corrupt, None),
                Err(HistoryError::Unavailable),
                "impossible approval tuple must fail closed: {corrupt}"
            );
        }
    }

    #[test]
    fn tool_result_decodes_canonical_identities_strictly() {
        let valid = serde_json::json!({
            "attempt_id": "00000000-0000-0000-0000-000000000001",
            "status": "failed",
            "code": "attempt_limit",
            "effect_state": "started",
            "output_bytes": 0,
            "output_digest": null,
            "version": 3,
        });
        assert_eq!(
            decode_payload("tool_result", &valid, None),
            Ok(ItemPayload::ToolResult {
                attempt_id: Some(crate::domain::execution::AttemptId::from_uuid(
                    uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("valid")
                )),
                status: crate::domain::execution::ExecutionStatus::Failed,
                code: Some("attempt_limit".to_owned()),
                effect_state: Some(crate::domain::ToolEffectState::Started),
                output_bytes: 0,
                output_digest: None,
                version: Some(3),
            })
        );

        let denied = serde_json::json!({
            "attempt_id": null,
            "status": "failed",
            "code": "descriptor_missing",
            "effect_state": null,
            "output_bytes": 0,
            "output_digest": null,
            "version": null,
        });
        assert_eq!(
            decode_payload("tool_result", &denied, None),
            Ok(ItemPayload::ToolResult {
                attempt_id: None,
                status: crate::domain::execution::ExecutionStatus::Failed,
                code: Some("descriptor_missing".to_owned()),
                effect_state: None,
                output_bytes: 0,
                output_digest: None,
                version: None,
            })
        );

        // Absent members, wrong types, and malformed identities fail decoding
        // instead of being silently reinterpreted as pre-D-7 or successful
        // views (TC-06).
        for corrupt in [
            serde_json::json!({"status": "failed", "code": null, "output_bytes": 0}),
            serde_json::json!({"attempt_id": 7, "status": "failed", "code": null, "output_bytes": 0}),
            serde_json::json!({"attempt_id": "not-a-uuid", "status": "failed", "code": null, "output_bytes": 0}),
            serde_json::json!({"attempt_id": null, "status": "failed", "output_bytes": 0}),
            serde_json::json!({"attempt_id": null, "status": "failed", "code": 4, "output_bytes": 0}),
            // Impossible canonical states fail decoding instead of becoming
            // trusted replay history (TC-06).
            serde_json::json!({"attempt_id": null, "status": "prepared", "code": null, "effect_state": null, "output_bytes": 0, "version": null}),
            serde_json::json!({"attempt_id": null, "status": "running", "code": null, "effect_state": null, "output_bytes": 0, "version": null}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "succeeded", "code": "attempt_limit", "effect_state": "started", "output_bytes": 3, "version": 3}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "failed", "code": null, "effect_state": "started", "output_bytes": 0, "version": 3}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "failed", "code": "attempt_limit", "effect_state": null, "output_bytes": 0, "version": 3}),
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "failed", "code": "attempt_limit", "effect_state": "started", "output_bytes": 0, "version": null}),
            serde_json::json!({"attempt_id": null, "status": "failed", "code": "descriptor_missing", "effect_state": null, "output_bytes": 0, "version": 0}),
            // A pre-D-7 record is only the typed denial shape: a successful
            // result without a canonical D-7 identity, or a denial with a
            // nonzero output size, is corrupt (TC-06).
            serde_json::json!({"attempt_id": null, "status": "succeeded", "code": null, "effect_state": null, "output_bytes": 0, "version": null}),
            serde_json::json!({"attempt_id": null, "status": "failed", "code": "descriptor_missing", "effect_state": null, "output_bytes": 5, "version": null}),
            // An executed terminal must carry the canonical terminal
            // transition version 3 (projection contract).
            serde_json::json!({"attempt_id": "00000000-0000-0000-0000-000000000001", "status": "succeeded", "code": null, "effect_state": "started", "output_bytes": 3, "version": 2}),
        ] {
            assert_eq!(
                decode_payload("tool_result", &corrupt, None),
                Err(HistoryError::Unavailable),
                "corrupt canonical identity must fail closed: {corrupt}"
            );
        }
    }

    #[test]
    fn tool_call_decodes_exact_transition_versions() {
        // The dispatch view carries exact canonical transition versions:
        // `prepared` = 1 and `running` = 2 (projection contract, TC-06).
        let valid = |status: &str, version| {
            serde_json::json!({
                "descriptor_id": "fixture.tool",
                "descriptor_version": "v1",
                "target": "fixture-target",
                "attempt_id": "00000000-0000-0000-0000-000000000001",
                "status": status,
                "version": version,
            })
        };
        for (status, version) in [("prepared", 1), ("running", 2)] {
            assert!(
                matches!(
                    decode_payload("tool_call", &valid(status, version), None),
                    Ok(ItemPayload::ToolCall { .. })
                ),
                "canonical {status}/v{version} dispatch view must replay"
            );
        }
        for (status, version) in [("prepared", 2), ("running", 1), ("running", 3)] {
            assert_eq!(
                decode_payload("tool_call", &valid(status, version), None),
                Err(HistoryError::Unavailable),
                "a noncanonical {status}/v{version} transition version must fail closed"
            );
        }
    }

    #[test]
    fn tool_call_replay_rejects_noncanonical_descriptor_fields() {
        let valid = serde_json::json!({
            "descriptor_id": "fixture.tool",
            "descriptor_version": "v1",
            "target": "fixture-target",
            "attempt_id": "00000000-0000-0000-0000-000000000001",
            "status": "running",
            "version": 2,
        });
        assert!(matches!(
            decode_payload("tool_call", &valid, None),
            Ok(ItemPayload::ToolCall { .. })
        ));

        for (field, value) in [
            ("descriptor_id", serde_json::json!("\u{7}")),
            ("descriptor_version", serde_json::json!("版本")),
            ("target", serde_json::json!("\t")),
        ] {
            let mut corrupt = valid.clone();
            corrupt[field] = value;
            assert_eq!(
                decode_payload("tool_call", &corrupt, None),
                Err(HistoryError::Unavailable),
                "noncanonical {field} must not enter replay history"
            );
        }
    }

    #[test]
    fn pre_d7_denial_tool_call_replays_with_unresolved_descriptor_fields() {
        let denied = serde_json::json!({
            "descriptor_id": "",
            "descriptor_version": "",
            "target": "",
            "attempt_id": null,
            "status": null,
            "version": null,
        });

        assert!(matches!(
            decode_payload("tool_call", &denied, None),
            Ok(ItemPayload::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id: None,
                status: None,
                version: None,
            }) if descriptor_id.is_empty() && descriptor_version.is_empty() && target.is_empty()
        ));
    }
}

/// Decodes a canonical optional enum field under the strict presence contract.
pub(super) fn required_optional_enum<T>(
    payload: &Value,
    name: &str,
    parse: fn(&str) -> Result<T, HistoryError>,
) -> Result<Option<T>, HistoryError> {
    match payload.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => parse(value).map(Some),
        _ => Err(HistoryError::Unavailable),
    }
}

/// Decodes a canonical optional number field under the strict presence contract.
pub(super) fn required_optional_u64(
    payload: &Value,
    name: &str,
) -> Result<Option<u64>, HistoryError> {
    match payload.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or(HistoryError::Unavailable),
        _ => Err(HistoryError::Unavailable),
    }
}

fn effect_state_name(state: crate::domain::ToolEffectState) -> &'static str {
    match state {
        crate::domain::ToolEffectState::NotStarted => "not_started",
        crate::domain::ToolEffectState::Started => "started",
        crate::domain::ToolEffectState::Unknown => "unknown",
    }
}

pub(super) fn parse_effect_state(
    value: &str,
) -> Result<crate::domain::ToolEffectState, HistoryError> {
    match value {
        "not_started" => Ok(crate::domain::ToolEffectState::NotStarted),
        "started" => Ok(crate::domain::ToolEffectState::Started),
        "unknown" => Ok(crate::domain::ToolEffectState::Unknown),
        _ => Err(HistoryError::Unavailable),
    }
}
