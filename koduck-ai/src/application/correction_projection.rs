// ADR: koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md

//! The C-2 scoped effective correction projection (ADR-0005 EP-01 through
//! EP-08): one pure, synchronous transformation from one explicitly scoped
//! Turn replay to the borrowed effective view of every non-correction Item.
//! Raw replay stays canonical; this module owns chain interpretation, source
//! validation, output order, and typed rejection, and it performs no I/O,
//! logging, persistence, or provider conversion.

use std::collections::HashMap;

use thiserror::Error;

use crate::domain::item_correction::{RawReplayStructureError, validate_raw_replay_refs};
use crate::domain::{DomainValueError, Item, ItemId, ItemPayload, TenantId, ThreadId, TurnId};

/// The explicit provenance one authenticated source reported for a projected
/// row (ADR-0005 EP-01): the tenant, subject, Thread, and Turn that own the
/// borrowed `Item`. These components are compared, never fetched, inferred
/// from content, or treated as credentials; the later integrating store
/// boundary remains responsible for truthful provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionScope {
    tenant: TenantId,
    subject: String,
    thread: ThreadId,
    turn: TurnId,
}

impl ProjectionScope {
    /// Creates a projection scope from already validated identity components
    /// without accepting a new external identity format.
    ///
    /// # Errors
    ///
    /// Returns [`DomainValueError::Empty`] when the subject identifier is blank.
    pub fn new(
        tenant: TenantId,
        subject: impl Into<String>,
        thread: ThreadId,
        turn: TurnId,
    ) -> Result<Self, DomainValueError> {
        let subject = subject.into();
        if subject.trim().is_empty() {
            Err(DomainValueError::Empty {
                field: "subject_id",
            })
        } else {
            Ok(Self {
                tenant,
                subject,
                thread,
                turn,
            })
        }
    }
}

/// One projection input entry (ADR-0005 EP-01): one canonical [`Item`]
/// borrowed from the caller plus the [`ProjectionScope`] its authenticated
/// source reported for that row.
#[derive(Clone, Copy, Debug)]
pub struct ScopedProjectionItem<'a> {
    item: &'a Item,
    scope: &'a ProjectionScope,
}

impl<'a> ScopedProjectionItem<'a> {
    /// Wraps one canonical Item with the scope its source reported.
    #[must_use]
    pub const fn new(item: &'a Item, scope: &'a ProjectionScope) -> Self {
        Self { item, scope }
    }

    /// Returns the borrowed canonical Item with its exact identity, sequence,
    /// and payload.
    #[must_use]
    pub const fn item(&self) -> &'a Item {
        self.item
    }

    /// Returns the borrowed source scope reported for this row.
    #[must_use]
    pub const fn scope(&self) -> &'a ProjectionScope {
        self.scope
    }
}

/// One borrowed output view (ADR-0005 EP-04): the original non-correction
/// Item at its preserved position, the selected effective source (the
/// original itself, or the last correction in that root's chain), and the
/// effective text content when the root is textual. The view is distinct
/// from a durable `Item` and introduces no persistence or wire conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveItem<'a> {
    original: &'a Item,
    source: &'a Item,
    effective_content: Option<&'a str>,
}

impl<'a> EffectiveItem<'a> {
    /// Returns the original non-correction Item with its exact identity,
    /// sequence, and payload kind.
    #[must_use]
    pub const fn original(&self) -> &'a Item {
        self.original
    }

    /// Returns the Item selected as this position's effective source: the
    /// original itself, or the exact last correction in its chain.
    #[must_use]
    pub const fn source(&self) -> &'a Item {
        self.source
    }

    /// Returns the effective text content borrowed from the selected source
    /// payload, or `None` for non-text kinds.
    #[must_use]
    pub const fn effective_content(&self) -> Option<&'a str> {
        self.effective_content
    }
}

/// A typed projection rejection (ADR-0005 EP-06). The projection is atomic:
/// no partial prefix, fallback, or silently dropped malformed chain is ever
/// returned, and no variant carries original or replacement content,
/// tenant/subject text, or any raw payload.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ProjectionError {
    /// The first mismatching entry does not belong to the expected scope
    /// (EP-01). Only its zero-based input index is reported; scope values are
    /// never carried.
    #[error("projection input entry {index} is outside the expected scope")]
    ScopeMismatch {
        /// Zero-based input index of the first mismatching entry.
        index: usize,
    },
    /// The supplied replay violates the raw structure contract (EP-02). The
    /// exact validator cause stays inspectable.
    #[error("raw replay structure is invalid")]
    InvalidReplay(
        /// The exact raw-replay structural cause.
        RawReplayStructureError,
    ),
    /// A correction targets a strictly later Item in the same Turn (EP-03).
    #[error("correction targets a later item in the same turn")]
    ForwardReference,
    /// A correction chain terminates at a kind other than a user message or
    /// agent message delta (EP-03).
    #[error("correction chain terminates at an unsupported root kind")]
    UnsupportedRoot,
}

/// Projects one explicitly scoped Turn replay to the borrowed effective view
/// of every non-correction Item (ADR-0005 EP-01 through EP-08).
///
/// The supplied slice must represent the complete source Turn replay at the
/// caller's chosen read point: this pure function neither fetches missing
/// rows nor certifies completeness, source authenticity, or freshness, and
/// flat multi-Turn history must not be relabeled as one Turn to satisfy this
/// contract. Validation passes run in the declared order — scope, raw
/// structure, then ordered ancestry — and each pass reports its first
/// violation. On success the output keeps the original input order with
/// exactly one view per non-correction Item and no separate correction
/// element; the input is never mutated.
///
/// # Errors
///
/// Returns [`ProjectionError::ScopeMismatch`] for the first foreign entry,
/// [`ProjectionError::InvalidReplay`] with the exact raw validator cause,
/// [`ProjectionError::ForwardReference`] for a strictly later correction
/// target, or [`ProjectionError::UnsupportedRoot`] when a chain terminates
/// at any other kind.
pub fn project_corrections<'a>(
    expected_scope: &ProjectionScope,
    entries: &'a [ScopedProjectionItem<'a>],
) -> Result<Vec<EffectiveItem<'a>>, ProjectionError> {
    reject_foreign_scope(expected_scope, entries)?;
    validate_raw_replay_refs(entries.iter().map(ScopedProjectionItem::item))
        .map_err(ProjectionError::InvalidReplay)?;

    let mut projection = ChainProjection::with_capacity(entries.len());
    for entry in entries {
        projection.absorb(entry.item())?;
    }
    Ok(projection.finish())
}

/// Rejects the first entry whose reported scope differs from the expected
/// component-wise (EP-01), before any structural validation.
fn reject_foreign_scope(
    expected_scope: &ProjectionScope,
    entries: &[ScopedProjectionItem<'_>],
) -> Result<(), ProjectionError> {
    for (index, entry) in entries.iter().enumerate() {
        let reported = entry.scope();
        if reported.tenant != expected_scope.tenant
            || reported.subject != expected_scope.subject
            || reported.thread != expected_scope.thread
            || reported.turn != expected_scope.turn
        {
            return Err(ProjectionError::ScopeMismatch { index });
        }
    }
    Ok(())
}

/// The effective source chain of one non-correction Item: absent when the
/// Item is its own uncorrected root, or the chain root's output position.
enum ChainRoot {
    /// A user message or agent message delta root at this output position.
    Textual(usize),
    /// A correction already resolved to its chain root's output position.
    Corrected(usize),
    /// A non-correction Item that no correction may target.
    NonTextual,
}

/// Call-local projection state (EP-07): output slots in input order plus one
/// `ItemId` index from every absorbed entry to its chain-root reference. The
/// single forward pass bounds retained metadata by O(n); no recursion, no
/// per-root history scan, and no payload copying exists here.
struct ChainProjection<'a> {
    slots: Vec<(&'a Item, &'a Item)>,
    chains: HashMap<ItemId, ChainRoot>,
}

impl<'a> ChainProjection<'a> {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            chains: HashMap::with_capacity(capacity),
        }
    }

    /// Absorbs one input entry in input order, inspecting each correction at
    /// its own position so the first ancestry violation in input order wins
    /// (EP-03).
    fn absorb(&mut self, item: &'a Item) -> Result<(), ProjectionError> {
        let position = self.slots.len();
        if let ItemPayload::Correction(correction) = &item.payload {
            let root = match self.chains.get(&correction.corrects_item_id()) {
                // Every earlier entry is indexed, so an absent predecessor is
                // strictly later in this Turn.
                None => return Err(ProjectionError::ForwardReference),
                Some(ChainRoot::NonTextual) => return Err(ProjectionError::UnsupportedRoot),
                Some(ChainRoot::Textual(slot) | ChainRoot::Corrected(slot)) => *slot,
            };
            // Deterministic chain-tip substitution: the latest correction in
            // input order becomes the root's selected source.
            self.slots[root].1 = item;
            self.chains.insert(item.item_id, ChainRoot::Corrected(root));
        } else {
            self.slots.push((item, item));
            match &item.payload {
                ItemPayload::UserMessage { .. } | ItemPayload::AgentMessageDelta { .. } => {
                    self.chains
                        .insert(item.item_id, ChainRoot::Textual(position));
                }
                _ => {
                    self.chains.insert(item.item_id, ChainRoot::NonTextual);
                }
            }
        }
        Ok(())
    }

    /// Finalizes every output slot after the complete input validated: the
    /// effective content is read from the selected source payload without
    /// trimming, length rechecking, joining, or cloning (EP-04, EP-05).
    fn finish(self) -> Vec<EffectiveItem<'a>> {
        self.slots
            .into_iter()
            .map(|(original, source)| EffectiveItem {
                original,
                source,
                effective_content: effective_content(source),
            })
            .collect()
    }
}

/// Returns the effective text content of one selected source payload: the
/// correction replacement for a chain tip, the plain content for uncorrected
/// text roots, and `None` for every other kind.
fn effective_content(source: &Item) -> Option<&str> {
    match &source.payload {
        ItemPayload::UserMessage { content } | ItemPayload::AgentMessageDelta { content } => {
            Some(content.as_str())
        }
        ItemPayload::Correction(correction) => Some(correction.content()),
        _ => None,
    }
}
