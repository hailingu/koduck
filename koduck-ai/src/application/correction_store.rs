// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md

//! Consumer-owned port for one authenticated post-terminal correction
//! admission.

use thiserror::Error;

use crate::domain::item_correction::ItemCorrection;
use crate::domain::{Item, ItemId, ThreadId, TrustContext, TurnId};

/// The exact UTF-8 byte ceiling of one correction's replacement content,
/// measured before serialization (ADR-0004 CA-01).
pub const MAX_CORRECTION_CONTENT_BYTES: usize = 65_536;

/// One authenticated correction admission was rejected or could not be
/// resolved (ADR-0004 CA-01 through CA-09).
///
/// Every variant other than [`CorrectionError::Unavailable`] is a proven
/// outcome: the invocation made no new durable change, and an exact retry
/// may still observe a previous invocation's existing Item.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CorrectionError {
    /// The admission could not complete within its bounded budget, or the
    /// commit outcome remained unknown after one reconciliation attempt
    /// (CA-07). Never describes zero mutation.
    #[error("correction admission unavailable")]
    Unavailable,
    /// The replacement content is blank or exceeds
    /// [`MAX_CORRECTION_CONTENT_BYTES`] (CA-01).
    #[error("correction content is invalid")]
    InvalidContent,
    /// The correction identity equals its predecessor, or the predecessor is
    /// missing, cross-scope, unsupported, or structurally inadmissible
    /// (CA-01/CA-03).
    #[error("correction predecessor is invalid")]
    InvalidPredecessor,
    /// The tenant, Thread, Turn, or subject does not match an existing owned
    /// Turn; non-owned targets are indistinguishable from missing ones
    /// (CA-02).
    #[error("owned turn was not found")]
    NotFound,
    /// The owned Turn has not reached a terminal state that admits a
    /// correction (CA-02).
    #[error("turn is not terminal")]
    TurnNotTerminal,
    /// The otherwise valid predecessor tip already has one direct correction
    /// successor (CA-03).
    #[error("correction predecessor already has one successor")]
    PredecessorConflict,
    /// The caller-stable identity exists in the tenant with a different
    /// scope, kind, predecessor, or content (CA-04).
    #[error("correction identity conflicts with its canonical record")]
    IdentityConflict,
    /// Durable correction state is malformed or contradicts an owned
    /// invariant, such as a malformed stored payload, a broken ancestor
    /// link, a cycle, a nondecreasing ancestor order, or an invalid sequence
    /// counter (CA-03 through CA-05).
    #[error("durable correction history is corrupt")]
    CorruptHistory,
    /// The valid admission exceeded the bounded ancestor count or a stored
    /// payload exceeded its read cap (CA-06).
    #[error("correction admission exceeded a resource bound")]
    ResourceLimit,
    /// The write settled with no durable Item and one bounded read-only
    /// reconciliation proved its absence (CA-07).
    #[error("correction was proven not applied")]
    NotApplied,
}

/// One authenticated correction command: the validated replacement content
/// plus every ownership and identity dimension (ADR-0004 CA-01).
///
/// Construction validates the complete command-local contract, so a stored
/// command can never carry blank content, content above
/// [`MAX_CORRECTION_CONTENT_BYTES`], or a self-referential predecessor. The
/// caller allocates the correction identity once and retains it across
/// retries; no sequence or payload discriminator is caller-controlled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrectionCommand {
    trust: TrustContext,
    thread_id: ThreadId,
    turn_id: TurnId,
    item_id: ItemId,
    correction: ItemCorrection,
}

impl CorrectionCommand {
    /// Validates and binds one correction command.
    ///
    /// # Errors
    ///
    /// Returns [`CorrectionError::InvalidContent`] when the replacement
    /// content is blank or exceeds [`MAX_CORRECTION_CONTENT_BYTES`] UTF-8
    /// bytes, and [`CorrectionError::InvalidPredecessor`] when the correction
    /// identity equals its predecessor identity.
    pub fn new(
        trust: TrustContext,
        thread_id: ThreadId,
        turn_id: TurnId,
        item_id: ItemId,
        predecessor_item_id: ItemId,
        content: impl Into<String>,
    ) -> Result<Self, CorrectionError> {
        let correction = ItemCorrection::new(content, predecessor_item_id)
            .map_err(|_| CorrectionError::InvalidContent)?;
        if correction.content().len() > MAX_CORRECTION_CONTENT_BYTES {
            return Err(CorrectionError::InvalidContent);
        }
        if item_id == predecessor_item_id {
            return Err(CorrectionError::InvalidPredecessor);
        }
        Ok(Self {
            trust,
            thread_id,
            turn_id,
            item_id,
            correction,
        })
    }

    /// Returns the authenticated tenant and subject that own the target Turn.
    #[must_use]
    pub const fn trust(&self) -> &TrustContext {
        &self.trust
    }

    /// Returns the owned Thread identity of the target Turn.
    #[must_use]
    pub const fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// Returns the target Turn identity.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }

    /// Returns the caller-stable correction identity retained across retries.
    #[must_use]
    pub const fn item_id(&self) -> ItemId {
        self.item_id
    }

    /// Returns the corrected predecessor Item identity.
    #[must_use]
    pub const fn predecessor_item_id(&self) -> ItemId {
        self.correction.corrects_item_id()
    }

    /// Returns the validated replacement content with its bytes preserved.
    #[must_use]
    pub fn content(&self) -> &str {
        self.correction.content()
    }

    /// Returns the validated typed replacement.
    pub(crate) const fn correction(&self) -> &ItemCorrection {
        &self.correction
    }
}

/// Consumer-owned boundary for one authenticated post-terminal correction
/// admission (ADR-0004 CA-02 through CA-09).
///
/// The port owns no foreground authority: admission is one guarded durable
/// transaction against a terminal, subject-owned Turn. Ordinary foreground
/// append keeps rejecting terminal Turns, and a successful correction leaves
/// the Turn's status, terminal Item, interrupt flags, and lease fields
/// unchanged.
pub trait CorrectionStore {
    /// Admits one correction, or resolves its caller-stable identity.
    ///
    /// A durable exact retry returns the original committed Item — including
    /// its allocated sequence — without writing, even when a later correction
    /// has since succeeded it. Competing fresh identities for one tip yield
    /// exactly one success and [`CorrectionError::PredecessorConflict`] for
    /// the rest; identical concurrent requests return the same one durable
    /// Item.
    ///
    /// # Errors
    ///
    /// Returns the typed [`CorrectionError`] categories declared by
    /// ADR-0004 CA-01 through CA-09. [`CorrectionError::Unavailable`] never
    /// describes zero mutation; a proven absence is
    /// [`CorrectionError::NotApplied`].
    fn correct(&self, command: CorrectionCommand) -> Result<Item, CorrectionError>;
}
