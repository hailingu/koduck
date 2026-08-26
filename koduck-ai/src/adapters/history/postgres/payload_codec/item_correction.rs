// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! Durable correction Item encoding and fail-closed decoding (ADR-0003
//! CR-01/CR-05). The predecessor identity lives only in the durable
//! `corrects_item_id` relationship column; the canonical payload JSON
//! carries exactly the replacement content.

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::application::HistoryError;
use crate::domain::ItemId;
use crate::domain::item_correction::ItemCorrection;

/// The durable discriminator of one correction Item.
pub(super) const DISCRIMINATOR: &str = "correction";

/// Encodes the canonical correction payload JSON.
pub(super) fn encode(correction: &ItemCorrection) -> Value {
    json!({ "content": correction.content() })
}

/// The strict durable correction document: exactly one `content` string
/// member. Derived deserialization rejects a duplicated `content` member,
/// and `deny_unknown_fields` rejects every extra member, so the canonical
/// shape is enforced while the payload is constructed in one pass with no
/// intermediate full-document `Value` (ADR-0003 CR-05 and the
/// resource-bounds matrix).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrectionDocument {
    content: String,
}

/// Decodes one durable correction row from its raw payload text, failing
/// closed on every malformed shape (CR-05): the payload must deserialize
/// as exactly the one `content` string member, the durable relationship
/// column must be present, and the typed representation must validate. A
/// payload carrying any extra or duplicated member is a non-canonical
/// externally inserted shape and is never silently canonicalized into
/// replay history.
pub(super) fn decode(
    payload: &str,
    corrects_item_id: Option<Uuid>,
) -> Result<ItemCorrection, HistoryError> {
    let document: CorrectionDocument =
        serde_json::from_str(payload).map_err(|_| HistoryError::Unavailable)?;
    let corrects_item_id = corrects_item_id.ok_or(HistoryError::Unavailable)?;
    ItemCorrection::new(document.content, ItemId::from_uuid(corrects_item_id))
        .map_err(|_| HistoryError::Unavailable)
}
