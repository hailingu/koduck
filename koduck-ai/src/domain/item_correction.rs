// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md

//! The typed correction Item representation and the raw-replay structure
//! contract (ADR-0003 CR-01 through CR-05).

use std::collections::HashSet;

use thiserror::Error;

use super::{DomainValueError, Item, ItemId, ItemPayload};

/// The typed replacement content of one correction Item: one non-blank
/// replacement string plus the identity of the corrected predecessor Item
/// (ADR-0003 CR-01).
///
/// Only the representation invariant lives here; admission policy — content
/// limits, predecessor kind, and caller identity — is owned by CAND-11.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemCorrection {
    content: String,
    corrects_item_id: ItemId,
}

impl ItemCorrection {
    /// Creates a correction after enforcing the representation invariant.
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::Empty`] when the replacement content is blank.
    pub fn new(
        content: impl Into<String>,
        corrects_item_id: ItemId,
    ) -> Result<Self, DomainValueError> {
        let content = content.into();
        if content.trim().is_empty() {
            Err(DomainValueError::Empty { field: "content" })
        } else {
            Ok(Self {
                content,
                corrects_item_id,
            })
        }
    }

    /// Returns the validated replacement content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns the corrected predecessor Item identity.
    #[must_use]
    pub const fn corrects_item_id(&self) -> ItemId {
        self.corrects_item_id
    }
}

/// A structural violation of the immutable ordered raw-replay contract.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RawReplayStructureError {
    /// Item sequences were not strictly increasing.
    #[error("replay sequence is not strictly increasing")]
    NonIncreasingSequence,
    /// One Item identity appeared more than once.
    #[error("replay contains a duplicate item identity")]
    DuplicateItemIdentity,
    /// A correction target is absent from the replayed Turn.
    #[error("correction target is absent from the replayed turn")]
    UnknownCorrectionTarget,
    /// A correcting Item identified itself.
    #[error("correction item identifies itself")]
    SelfCorrection,
    /// One predecessor already has a direct correction successor.
    #[error("correction target already has one direct successor")]
    DuplicateSuccessor,
}

/// Validates the structure of one decoded raw Turn replay: every Item
/// appears exactly once in strictly increasing sequence order, and every
/// correction relationship stays inside the replayed Turn, never identifies
/// the correcting Item itself, and grants its predecessor at most one direct
/// successor (ADR-0003 CR-02 through CR-05).
///
/// Sequence adjacency is not required, and target ordering is deliberately
/// not judged here: CAND-11 admission owns predecessor currency and CAND-12
/// owns effective-order semantics. Validation never mutates the replay.
///
/// # Errors
///
/// Returns the first [`RawReplayStructureError`] violation, if any.
pub fn validate_raw_replay(items: &[Item]) -> Result<(), RawReplayStructureError> {
    validate_replay_order(items)?;
    validate_correction_structure(items)
}

/// Requires every Item identity to appear exactly once in strictly increasing
/// sequence order (ADR-0003 CR-02).
///
/// Sequence adjacency is not required. Validation never mutates the replay.
fn validate_replay_order(items: &[Item]) -> Result<(), RawReplayStructureError> {
    let mut identities = HashSet::with_capacity(items.len());
    let mut previous_sequence = 0_u64;
    for item in items {
        if item.sequence <= previous_sequence {
            return Err(RawReplayStructureError::NonIncreasingSequence);
        }
        previous_sequence = item.sequence;
        if !identities.insert(item.item_id) {
            return Err(RawReplayStructureError::DuplicateItemIdentity);
        }
    }
    Ok(())
}

/// Requires every correction relationship to stay inside the replayed Turn,
/// never identify the correcting Item itself, and grant its predecessor at
/// most one direct successor (ADR-0003 CR-03 through CR-05).
///
/// Target ordering is deliberately not judged here: CAND-11 admission owns
/// predecessor currency and CAND-12 owns effective-order semantics.
fn validate_correction_structure(items: &[Item]) -> Result<(), RawReplayStructureError> {
    let identities: HashSet<ItemId> = items.iter().map(|item| item.item_id).collect();
    let mut targets = HashSet::new();
    for item in items {
        if let ItemPayload::Correction(correction) = &item.payload {
            let target = correction.corrects_item_id();
            if target == item.item_id {
                return Err(RawReplayStructureError::SelfCorrection);
            }
            if !identities.contains(&target) {
                return Err(RawReplayStructureError::UnknownCorrectionTarget);
            }
            if !targets.insert(target) {
                return Err(RawReplayStructureError::DuplicateSuccessor);
            }
        }
    }
    Ok(())
}
