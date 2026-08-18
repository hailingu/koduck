// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! `JSON` serialization for bounded C-5 audit records (ADR-0003 TC-14).

use crate::application::{ToolAuditRecord, ToolAuditRecordTooLarge};

/// Serializes one audit record within the TC-14 byte bound.
///
/// # Errors
///
/// Returns [`ToolAuditRecordTooLarge`] when the serialized record would
/// exceed [`MAX_AUDIT_RECORD_BYTES`]; the caller then emits no record rather
/// than truncating correlated evidence.
pub fn serialize_audit_record(record: &ToolAuditRecord) -> Result<String, ToolAuditRecordTooLarge> {
    let serialized = serde_json::to_string(record).map_err(|_| ToolAuditRecordTooLarge)?;
    record.serialized_within_bound(serialized)
}
