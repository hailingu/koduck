// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md

//! The provider-context preparation owner (ADR-0006 PC-02 through PC-04):
//! one pure, synchronous transformation from validated complete per-Turn
//! provenance groups to the owned effective provider-history input. Group
//! structure is checked first, the unchanged CAND-12 projection runs once per
//! source Turn, and its borrowed views convert into one owned effective view
//! per non-correction Item. Raw history admission, canonical writes, and
//! provider I/O stay outside this module.

use std::collections::HashSet;

use thiserror::Error;

use crate::domain::{Item, ItemId, ItemPayload, ThreadId, TrustContext, TurnId};

use super::correction_projection::{
    EffectiveItem, ProjectionError, ProjectionScope, ScopedProjectionItem, project_corrections,
};

/// One canonical prior-Turn row together with the provenance its
/// authenticated source reported for it (ADR-0006 PC-01): the canonical
/// [`Item`] exactly as the bounded read returned it, plus the scope against
/// which the projection's own scope validation compares it. The scope is
/// independent of the group's explicit `source_turn`, so a test double can
/// supply a row/group Turn mismatch even at row index 0.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriorTurnRow {
    /// Canonical Item exactly as the authenticated read returned it.
    pub item: Item,
    /// Scope the authenticated source reported for this row.
    pub scope: ProjectionScope,
}

impl PriorTurnRow {
    /// Wraps one canonical Item with the scope its source reported.
    #[must_use]
    pub const fn new(item: Item, scope: ProjectionScope) -> Self {
        Self { item, scope }
    }
}

/// One complete ordered source-Turn group from the authenticated bounded
/// prior-history read (ADR-0006 PC-01). The explicit `source_turn` is the
/// Turn identity the adapter used to group these rows; preparation builds the
/// expected projection scope from this field and never derives the expected
/// Turn from the group's first row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriorTurnHistory {
    /// Turn identity the adapter used to group these rows.
    pub source_turn: TurnId,
    /// The group's complete ordered rows with their reported scopes.
    pub rows: Vec<PriorTurnRow>,
}

impl PriorTurnHistory {
    /// Creates one source-Turn group over its complete ordered rows.
    #[must_use]
    pub const fn new(source_turn: TurnId, rows: Vec<PriorTurnRow>) -> Self {
        Self { source_turn, rows }
    }
}

/// The payload-kind semantics one provider view retains from its original
/// Item (ADR-0006 PC-04).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderHistoryKind {
    /// The authenticated user's plain-text root at its original position.
    UserMessage,
    /// One model delta root at its original position.
    AgentMessageDelta,
    /// Provider token accounting view; inert under provider translation.
    Usage,
    /// D-3 approval status view; inert under provider translation.
    ApprovalStatus,
    /// D-3 Tool call view; inert and never dispatch authority.
    ToolCall,
    /// D-3 Tool result view; inert under provider translation.
    ToolResult,
    /// One terminal boundary that flushes preceding assistant text.
    Terminal,
    /// Never produced by preparation: the projection absorbs corrections
    /// into their roots, so no view ever represents a correction row.
    Correction,
}

/// The owned effective value of one provider-history view (ADR-0006 PC-04):
/// the effective text copied from the selected source for a textual root,
/// or the unchanged non-text payload copied from the original Item. The
/// value is data only — it never carries approval or dispatch authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderHistoryValue {
    /// Effective text of a user or assistant root: the plain content, or the
    /// exact replacement of its last correction.
    Text(String),
    /// The unchanged non-text payload of the original Item: usage counters,
    /// approval-status, Tool-call/result views, or a terminal outcome.
    NonText(ItemPayload),
}

/// One owned effective provider-history view (ADR-0006 PC-04): the original
/// non-correction Item's identity, sequence, and payload-kind semantics at
/// its preserved position, the selected effective source's identity, and the
/// effective content or non-text value copied once from the CAND-12
/// accessors. The view is distinct from a canonical [`Item`]: it is provider
/// input only, never a persistence append input, REST/SSE document, or
/// execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderHistoryItem {
    /// Original Item identity at its preserved position.
    pub item_id: ItemId,
    /// Original Turn-local sequence at its preserved position.
    pub sequence: u64,
    /// Original payload-kind semantics used by provider translation.
    pub kind: ProviderHistoryKind,
    /// Identity of the selected effective source: the original itself, or
    /// the exact last correction in its chain.
    pub source_item_id: ItemId,
    /// The effective value copied once from the selected source: replacement
    /// text for textual roots, or the unchanged non-text payload.
    pub value: ProviderHistoryValue,
}

impl ProviderHistoryItem {
    /// Returns the effective text when this view is textual.
    #[must_use]
    pub const fn effective_text(&self) -> Option<&str> {
        match &self.value {
            ProviderHistoryValue::Text(content) => Some(content.as_str()),
            ProviderHistoryValue::NonText(_) => None,
        }
    }
}

/// A typed pre-acceptance provider-context rejection (ADR-0006 PC-07). No
/// variant carries original or replacement payloads, tenant/subject values,
/// raw rows, or credentials.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderContextError {
    /// The CAND-12 projection rejected one source Turn; the exact cause is
    /// preserved unchanged, including row-scope mismatch.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    /// A group was empty, or one `source_turn` occurred in more than one
    /// group.
    #[error("prior turn history provenance is invalid")]
    InvalidProvenance,
}

/// Validates complete per-Turn groups and prepares the owned effective
/// provider-history view (ADR-0006 PC-02 through PC-04).
///
/// Group structure is checked first: every group must be nonempty and each
/// `source_turn` may occur in exactly one group, so splitting or interleaving
/// one Turn into multiple groups rejects before any projection. Each group is
/// then projected through the unchanged CAND-12 owner, whose expected scope is
/// built from the validated command's tenant, subject, and Thread plus the
/// group's explicit `source_turn`; every row's reported scope passes through
/// unchanged, and CAND-12 alone validates it. Preparation is atomic: a
/// failure in any group returns no partial view, and success performs no
/// canonical write or provider call.
///
/// # Errors
///
/// Returns [`ProviderContextError::InvalidProvenance`] for an empty group or
/// a duplicate `source_turn`, and [`ProviderContextError::Projection`] with
/// the exact CAND-12 cause — including row-scope mismatch with its first
/// group-local index — otherwise.
pub fn prepare_provider_history(
    trust: &TrustContext,
    thread_id: ThreadId,
    groups: &[PriorTurnHistory],
) -> Result<Vec<ProviderHistoryItem>, ProviderContextError> {
    reject_invalid_provenance(groups)?;
    let mut prepared = Vec::new();
    for group in groups {
        project_group(trust, thread_id, group, &mut prepared)?;
    }
    Ok(prepared)
}

/// Rejects empty groups and duplicate `source_turn` identities before any
/// projection (PC-02).
fn reject_invalid_provenance(groups: &[PriorTurnHistory]) -> Result<(), ProviderContextError> {
    let mut source_turns = HashSet::with_capacity(groups.len());
    for group in groups {
        if group.rows.is_empty() || !source_turns.insert(group.source_turn) {
            return Err(ProviderContextError::InvalidProvenance);
        }
    }
    Ok(())
}

/// Projects one validated group through the unchanged CAND-12 owner and
/// appends its owned views in group order (PC-02).
fn project_group(
    trust: &TrustContext,
    thread_id: ThreadId,
    group: &PriorTurnHistory,
    prepared: &mut Vec<ProviderHistoryItem>,
) -> Result<(), ProviderContextError> {
    // A validated command subject is non-blank by TrustContext construction,
    // so the expected scope never fails to build here.
    let expected = ProjectionScope::new(
        trust.tenant_id.clone(),
        trust.subject_id.as_str(),
        thread_id,
        group.source_turn,
    )
    .expect("the validated command subject is non-blank by construction");
    let entries: Vec<_> = group
        .rows
        .iter()
        .map(|row| ScopedProjectionItem::new(&row.item, &row.scope))
        .collect();
    for view in project_corrections(&expected, &entries)? {
        prepared.push(provider_view(&view));
    }
    Ok(())
}

/// Converts one borrowed projection view into its owned provider view,
/// copying the selected effective value once (PC-04): text for textual
/// roots, the unchanged payload for every other kind.
fn provider_view(view: &EffectiveItem<'_>) -> ProviderHistoryItem {
    let original = view.original();
    let value = match view.effective_content() {
        Some(content) => ProviderHistoryValue::Text(content.to_owned()),
        None => ProviderHistoryValue::NonText(original.payload.clone()),
    };
    ProviderHistoryItem {
        item_id: original.item_id,
        sequence: original.sequence,
        kind: provider_history_kind(&original.payload),
        source_item_id: view.source().item_id,
        value,
    }
}

/// Maps one original payload to its retained provider kind semantics.
const fn provider_history_kind(payload: &ItemPayload) -> ProviderHistoryKind {
    match payload {
        ItemPayload::UserMessage { .. } => ProviderHistoryKind::UserMessage,
        ItemPayload::AgentMessageDelta { .. } => ProviderHistoryKind::AgentMessageDelta,
        ItemPayload::Usage(_) => ProviderHistoryKind::Usage,
        ItemPayload::ApprovalStatus { .. } => ProviderHistoryKind::ApprovalStatus,
        ItemPayload::ToolCall { .. } => ProviderHistoryKind::ToolCall,
        ItemPayload::ToolResult { .. } => ProviderHistoryKind::ToolResult,
        ItemPayload::Terminal(_) => ProviderHistoryKind::Terminal,
        ItemPayload::Correction(_) => ProviderHistoryKind::Correction,
    }
}
