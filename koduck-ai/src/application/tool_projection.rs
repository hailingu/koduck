// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Append-before-publish D-3 projections of canonical C-5 state (TC-06).

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::execution::{
    ApprovalDecision, ApprovalId, ApprovalStatus, AttemptId, ExecutionStatus,
};

use super::executor_envelope::{EffectState, ExecutionFailure};

mod stage;
mod validation;

use stage::ProjectionStage;
use validation::validate_canonical_tuple;

/// One append-only D-3 view of canonical D-6/D-7 state.
///
/// A projection carries its canonical identity and version and is published
/// only after its durable append succeeds; it is never authority and can never
/// be read back to authorize or redispatch execution (ADR-0003 TC-06).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolProjection {
    /// D-6 approval-status view at a canonical record version.
    ApprovalStatus {
        /// Canonical D-6 identity.
        approval_id: ApprovalId,
        /// Exact D-7 identity the D-6 authorizes or closes.
        attempt_id: AttemptId,
        /// Canonical status at this version.
        status: ApprovalStatus,
        /// Canonical decision, or `None` while requested, expired, or
        /// cancelled by an authenticated interruption.
        decision: Option<ApprovalDecision>,
        /// Canonical D-6 record version.
        version: u64,
    },
    /// D-7 dispatch-phase view.
    ToolCall {
        /// Descriptor identity the call addressed.
        descriptor_id: String,
        /// Descriptor version the call addressed.
        descriptor_version: String,
        /// Exact target the call addressed.
        target: String,
        /// Canonical D-7 identity.
        attempt_id: AttemptId,
        /// Canonical D-7 lifecycle phase at this version.
        status: ExecutionStatus,
        /// Canonical D-7 transition version.
        version: u64,
    },
    /// D-7 terminal-result view.
    ToolResult {
        /// Canonical D-7 identity.
        attempt_id: AttemptId,
        /// Canonical terminal lifecycle status.
        status: ExecutionStatus,
        /// Stable terminal failure code, or `None` for non-failed terminals.
        code: Option<ExecutionFailure>,
        /// Executor-observed effect state evidence.
        effect_state: EffectState,
        /// Serialized size of the bounded executor output.
        output_bytes: u64,
        /// SHA-256 digest binding a successful model continuation to its
        /// durable output, or `None` for every non-success terminal.
        output_digest: Option<String>,
        /// Canonical D-7 transition version.
        version: u64,
    },
    /// Typed pre-D-7 policy denial view: no D-6, no D-7, no dispatch.
    Denied {
        /// Descriptor identity the call addressed.
        descriptor_id: String,
        /// Descriptor version the call addressed, or empty when unresolved.
        descriptor_version: String,
        /// Exact target the call addressed, or empty when unresolved.
        target: String,
        /// Stable denial code.
        code: String,
    },
}

/// A D-3 projection append could not complete durably.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolProjectionError {
    /// The durable append did not complete within its availability contract.
    #[error("tool projection append unavailable")]
    Unavailable,
}

/// Consumer-owned append-before-publish boundary for D-3 projections.
///
/// `append` performs the durable append; `publish` makes the projection
/// externally visible and MAY be called only after `append` reported success
/// for the same value. A failed append suppresses publication but changes no
/// canonical D-6/D-7 state: the projection is a view, so authority and
/// dispatch decisions never depend on it (ADR-0003 TC-06).
pub trait ToolProjectionSink {
    /// Durably appends one projection.
    ///
    /// # Errors
    ///
    /// Returns [`ToolProjectionError`] when the durable append cannot complete.
    fn append(&mut self, projection: &ToolProjection) -> Result<(), ToolProjectionError>;

    /// Publishes one already durably appended projection.
    fn publish(&mut self, projection: &ToolProjection);

    /// Binds the stable non-UTF-8 model summary to its opaque committed bytes.
    ///
    /// The default keeps sinks that do not participate in runner continuation
    /// validation compatible. The live runner sink verifies the bytes against
    /// the already appended success projection before accepting the summary.
    fn bind_opaque_success_summary(&mut self, _output: &[u8]) {}
}

/// Explicit unconfigured projection boundary.
///
/// Appends succeed without durable effect and nothing is published, so C-5
/// callers that have not been wired to a D-3 history bridge still observe the
/// canonical outcomes directly; the runtime D-3 bridge replaces this sink when
/// the transport wiring lands.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoToolProjections;

impl ToolProjectionSink for NoToolProjections {
    fn append(&mut self, _projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        Ok(())
    }

    fn publish(&mut self, _projection: &ToolProjection) {}
}

/// Appends one projection and publishes it only when the append succeeded.
///
/// A failed append suppresses publication and changes no canonical D-6/D-7
/// state (ADR-0003 TC-06), but the failure is never concealed: it is reported
/// as a structured diagnostic so operators and reconciliation tooling can
/// observe the missing durable view.
#[allow(
    clippy::needless_pass_by_value,
    reason = "ownership documents the single-use emission of one projection"
)]
pub(crate) fn emit(sink: &mut dyn ToolProjectionSink, projection: ToolProjection) {
    match sink.append(&projection) {
        Ok(()) => sink.publish(&projection),
        Err(error) => {
            eprintln!(
                "event=tool_projection_append_failed error={error} projection_type={}",
                projection_kind(&projection)
            );
        }
    }
}

/// Returns the bounded diagnostic class for a projection append failure.
const fn projection_kind(projection: &ToolProjection) -> &'static str {
    match projection {
        ToolProjection::ApprovalStatus { .. } => "approval_status",
        ToolProjection::ToolCall { .. } => "tool_call",
        ToolProjection::ToolResult { .. } => "tool_result",
        ToolProjection::Denied { .. } => "denied",
    }
}

/// Canonical D-7 transition version for one lifecycle phase.
///
/// `prepared` is version 1, `running` version 2, and every terminal phase
/// version 3, so a projection sequence references strictly increasing
/// canonical versions along one attempt's transitions.
#[must_use]
pub(crate) const fn attempt_version(status: ExecutionStatus) -> u64 {
    match status {
        ExecutionStatus::Prepared => 1,
        ExecutionStatus::Running => 2,
        ExecutionStatus::Succeeded
        | ExecutionStatus::Failed
        | ExecutionStatus::TimedOut
        | ExecutionStatus::Cancelled => 3,
    }
}

/// Canonical D-6 record version for one approval state.
///
/// A request creates version 1 and its one terminal resolution creates
/// version 2, so replay and projection validation cannot accept an invented
/// approval history.
#[must_use]
pub(crate) const fn approval_version(status: ApprovalStatus) -> u64 {
    match status {
        ApprovalStatus::Requested => 1,
        ApprovalStatus::Accepted
        | ApprovalStatus::Declined
        | ApprovalStatus::Cancelled
        | ApprovalStatus::Expired => 2,
    }
}

impl ToolProjection {
    /// Converts one projection into its ordered append-only D-3 items.
    ///
    /// This is the only construction path from a projection to history
    /// items, so the wire shape of D-3 tool items is validated by
    /// construction and no unrestricted item batch crosses the
    /// tool-execution port (ADR-0003 TC-06).
    #[must_use]
    pub fn d3_items(&self) -> Vec<crate::application::NewItem> {
        use crate::application::NewItem;
        match self {
            Self::ApprovalStatus {
                approval_id,
                attempt_id,
                status,
                decision,
                version,
            } => vec![NewItem::ApprovalStatus {
                approval_id: *approval_id,
                attempt_id: *attempt_id,
                status: *status,
                decision: *decision,
                version: *version,
            }],
            Self::ToolCall {
                descriptor_id,
                descriptor_version,
                target,
                attempt_id,
                status,
                version,
            } => vec![NewItem::ToolCall {
                descriptor_id: descriptor_id.clone(),
                descriptor_version: descriptor_version.clone(),
                target: target.clone(),
                attempt_id: Some(*attempt_id),
                status: Some(*status),
                version: Some(*version),
            }],
            Self::ToolResult {
                attempt_id,
                status,
                code,
                effect_state,
                output_bytes,
                output_digest,
                version,
            } => vec![NewItem::ToolResult {
                attempt_id: Some(*attempt_id),
                status: *status,
                code: code.map(failure_code),
                effect_state: Some(tool_effect_state(*effect_state)),
                output_bytes: *output_bytes,
                output_digest: output_digest.clone(),
                version: Some(*version),
            }],
            Self::Denied {
                descriptor_id,
                descriptor_version,
                target,
                code,
            } => vec![
                NewItem::ToolCall {
                    descriptor_id: descriptor_id.clone(),
                    descriptor_version: descriptor_version.clone(),
                    target: target.clone(),
                    attempt_id: None,
                    status: None,
                    version: None,
                },
                NewItem::ToolResult {
                    attempt_id: None,
                    status: ExecutionStatus::Failed,
                    code: Some(code.clone()),
                    effect_state: None,
                    output_bytes: 0,
                    output_digest: None,
                    version: None,
                },
            ],
        }
    }
}

/// Converts an executor effect state into its D-3 mirror.
#[must_use]
pub(crate) fn tool_effect_state(state: EffectState) -> crate::domain::ToolEffectState {
    match state {
        EffectState::NotStarted => crate::domain::ToolEffectState::NotStarted,
        EffectState::Started => crate::domain::ToolEffectState::Started,
        EffectState::Unknown => crate::domain::ToolEffectState::Unknown,
    }
}

/// Returns the lower-case SHA-256 digest that binds one model continuation
/// result to the opaque bytes committed by its D-3 terminal projection.
#[must_use]
pub fn output_digest(output: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(output);
    format!("{:x}", hasher.finalize())
}

/// Converts a stable failure code into its D-3 text.
#[must_use]
fn failure_code(code: ExecutionFailure) -> String {
    code.stable_code().to_owned()
}

/// `TurnHistory`-backed durable projection sink for one serviced Tool call.
///
/// The sink is seeded with the runner's cumulative per-Turn provider counters
/// and synchronizes them back through [`Self::budget`], so one Turn's
/// projections share the single 64-item/1-MiB allowance with every provider
/// item instead of each call receiving a fresh budget (ADR-0001 exact buffer
/// contract). The port is untrusted: `append` first validates the
/// projection's canonical tuple, then reserves capacity for the complete
/// remaining lifecycle the projection opens — a running view is never
/// appended without capacity for its guaranteed terminal view, and a
/// requested approval never without capacity for its resolution, dispatch,
/// and terminal views — before the projection's own complete item sequence is
/// preflighted and atomically appended (ADR-0003 TC-06). `publish` forwards
/// the appended items to the live observer immediately, so a requested
/// approval or running transition is visible throughout the approval wait or
/// executor call. A rejected or failed append marks the sink failed and
/// suppresses publication; the runner surfaces the failure as a turn-level
/// durability failure.
pub struct TurnProjectionSink<'a, H> {
    history: &'a mut H,
    accepted: &'a crate::application::AcceptedTurn,
    observer: &'a mut dyn FnMut(crate::application::TurnStreamEvent),
    durable: Vec<crate::domain::Item>,
    observed_up_to: usize,
    item_count: usize,
    payload_bytes: usize,
    reserved_items: usize,
    reserved_bytes: usize,
    stage: ProjectionStage,
    committed_result: Option<CommittedProjectionResult>,
    opaque_success_summary: bool,
    failed: bool,
}

/// The final durable outcome that constrains the result handed back to the
/// provider for its continuation request.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CommittedProjectionResult {
    /// A success must return exactly the bytes committed by its terminal view.
    Succeeded {
        output_bytes: u64,
        output_digest: Option<String>,
    },
    /// A failed, timed-out, or cancelled attempt must return its stable model
    /// error summary.
    Failed(String),
    /// A typed policy denial must become a model error.
    Denied(String),
}

impl<'a, H> TurnProjectionSink<'a, H>
where
    H: crate::application::TurnHistory,
{
    /// Binds one sink to the live turn's history, accepted identity, and
    /// observer, seeded with the cumulative per-Turn budget counters.
    pub fn new(
        history: &'a mut H,
        accepted: &'a crate::application::AcceptedTurn,
        observer: &'a mut dyn FnMut(crate::application::TurnStreamEvent),
        item_count: usize,
        payload_bytes: usize,
    ) -> Self {
        Self {
            history,
            accepted,
            observer,
            durable: Vec::new(),
            observed_up_to: 0,
            item_count,
            payload_bytes,
            reserved_items: 0,
            reserved_bytes: 0,
            stage: ProjectionStage::Open,
            committed_result: None,
            opaque_success_summary: false,
            failed: false,
        }
    }

    /// Reports whether any projection was rejected or failed to append.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Reports whether the executor durably completed a canonical lifecycle.
    #[must_use]
    pub fn is_lifecycle_complete(&self) -> bool {
        matches!(
            self.stage,
            ProjectionStage::RetryAvailable | ProjectionStage::Complete | ProjectionStage::Denied
        )
    }

    /// Checks that the result offered for model continuation agrees with the
    /// final durable tool outcome.
    #[must_use]
    pub fn matches_committed_result(&self, result: &crate::application::ModelToolResult) -> bool {
        match &self.committed_result {
            Some(CommittedProjectionResult::Succeeded {
                output_bytes,
                output_digest: expected_digest,
            }) => {
                if self.opaque_success_summary {
                    result.is_error && result.content == "output_invalid_utf8"
                } else {
                    !result.is_error
                        && result.content.len() as u64 == *output_bytes
                        && expected_digest.as_deref().is_some_and(|digest| {
                            output_digest(result.content.as_bytes()) == digest
                        })
                }
            }
            Some(
                CommittedProjectionResult::Failed(expected)
                | CommittedProjectionResult::Denied(expected),
            ) => result.is_error && result.content == *expected,
            None => false,
        }
    }

    /// Returns the cumulative per-Turn budget counters after every appended
    /// projection, for the runner to synchronize back into its own state.
    /// Lifecycle reservations are per-call and released with the sink.
    #[must_use]
    pub fn budget(&self) -> (usize, usize) {
        (self.item_count, self.payload_bytes)
    }

    /// Publishes every durably appended item an implementation did not
    /// publish, so nothing durable stays invisible past the call boundary.
    pub fn drain_unpublished(&mut self) {
        self.observe_pending();
    }

    /// Releases the durable items for the runner's publication record.
    #[must_use]
    pub fn into_durable_items(self) -> Vec<crate::domain::Item> {
        self.durable
    }

    /// Observes every appended item not yet published, in append order.
    fn observe_pending(&mut self) {
        for item in &self.durable[self.observed_up_to..] {
            (self.observer)(crate::application::TurnStreamEvent::Item {
                thread_id: self.accepted.thread_id,
                turn_id: self.accepted.turn_id,
                item: item.clone(),
            });
        }
        self.observed_up_to = self.durable.len();
    }
}

/// Verifies that a history adapter acknowledged the exact planned D-3 batch.
fn matches_projection_acknowledgement(
    durable_items: &[crate::domain::Item],
    planned_payloads: &[crate::domain::ItemPayload],
) -> bool {
    durable_items.len() == planned_payloads.len()
        && durable_items.first().is_some_and(|item| item.sequence > 0)
        && durable_items
            .iter()
            .zip(planned_payloads)
            .all(|(durable, planned)| durable.payload == *planned)
        && durable_items.windows(2).all(|pair| {
            pair[0]
                .sequence
                .checked_add(1)
                .is_some_and(|next| pair[1].sequence == next)
        })
}

impl<H> ToolProjectionSink for TurnProjectionSink<'_, H>
where
    H: crate::application::TurnHistory,
{
    fn append(&mut self, projection: &ToolProjection) -> Result<(), ToolProjectionError> {
        // Once any projection was rejected or failed to append, the sink
        // stays failed: no later append may resume an incomplete lifecycle
        // and publish views whose earlier transitions never became durable
        // (projection contract, ADR-0003 TC-06).
        if self.failed {
            return Err(ToolProjectionError::Unavailable);
        }
        // The port is untrusted: reject noncanonical tuples and out-of-order
        // lifecycle projections before anything is planned or persisted.
        let items = projection.d3_items();
        let Some(plan) = validate_canonical_tuple(projection)
            .ok()
            .and_then(|()| self.stage.plan(projection))
        else {
            self.failed = true;
            return Err(ToolProjectionError::Unavailable);
        };
        // Preflight the projection's complete item sequence plus the held and
        // newly opened lifecycle reservations against the cumulative per-Turn
        // budgets before any part is appended: no partial prefix and no
        // orphan running or approval view can be left durable (ADR-0001
        // exact buffer contract, ADR-0003 TC-06).
        let policy = crate::application::AppendPolicy::cand_1();
        let mut next_count = self.item_count;
        let mut next_bytes = self.payload_bytes;
        for item in &items {
            next_count = next_count.saturating_add(1);
            let Ok(checked_bytes) = policy
                .check_item_count(next_count)
                .and_then(|()| policy.accumulate_payload_bytes(next_bytes, item))
            else {
                self.failed = true;
                return Err(ToolProjectionError::Unavailable);
            };
            next_bytes = checked_bytes;
        }
        let total_items = next_count
            .saturating_add(self.reserved_items)
            .saturating_sub(plan.release_items)
            .saturating_add(plan.reserve_items);
        let total_bytes = next_bytes
            .saturating_add(self.reserved_bytes)
            .saturating_sub(plan.release_bytes)
            .saturating_add(plan.reserve_bytes);
        if policy.check_item_count(total_items).is_err()
            || policy.check_payload_bytes(total_bytes).is_err()
        {
            self.failed = true;
            return Err(ToolProjectionError::Unavailable);
        }
        let planned_payloads = items
            .iter()
            .cloned()
            .map(crate::application::NewItem::into_payload)
            .collect::<Vec<_>>();
        let Ok(durable_items) = self.history.append_tool_projection(self.accepted, items) else {
            self.failed = true;
            return Err(ToolProjectionError::Unavailable);
        };
        if !matches_projection_acknowledgement(&durable_items, &planned_payloads) {
            self.failed = true;
            return Err(ToolProjectionError::Unavailable);
        }
        self.durable.extend(durable_items);
        self.item_count = next_count;
        self.payload_bytes = next_bytes;
        self.reserved_items = self
            .reserved_items
            .saturating_sub(plan.release_items)
            .saturating_add(plan.reserve_items);
        self.reserved_bytes = self
            .reserved_bytes
            .saturating_sub(plan.release_bytes)
            .saturating_add(plan.reserve_bytes);
        self.stage = plan.stage;
        self.opaque_success_summary = false;
        self.committed_result = match projection {
            ToolProjection::ToolResult {
                status: ExecutionStatus::Succeeded,
                output_bytes,
                output_digest,
                ..
            } => Some(CommittedProjectionResult::Succeeded {
                output_bytes: *output_bytes,
                output_digest: output_digest.clone(),
            }),
            ToolProjection::ToolResult { status, code, .. } => {
                let summary = match status {
                    ExecutionStatus::Failed => code
                        .expect("canonical failed projection has a code")
                        .stable_code()
                        .to_owned(),
                    ExecutionStatus::TimedOut => "timed_out".to_owned(),
                    ExecutionStatus::Cancelled => "cancelled".to_owned(),
                    ExecutionStatus::Prepared
                    | ExecutionStatus::Running
                    | ExecutionStatus::Succeeded => {
                        unreachable!("only terminal non-success projections reach this arm")
                    }
                };
                Some(CommittedProjectionResult::Failed(summary))
            }
            ToolProjection::Denied { code, .. } => {
                Some(CommittedProjectionResult::Denied(code.clone()))
            }
            ToolProjection::ApprovalStatus { .. } | ToolProjection::ToolCall { .. } => {
                self.committed_result.take()
            }
        };
        Ok(())
    }

    fn publish(&mut self, _projection: &ToolProjection) {
        // Publication is the visibility step: the appended items reach the
        // live observer now, in append order, not after the port returns.
        self.observe_pending();
    }

    fn bind_opaque_success_summary(&mut self, output: &[u8]) {
        self.opaque_success_summary = matches!(
            &self.committed_result,
            Some(CommittedProjectionResult::Succeeded {
                output_bytes,
                output_digest: Some(digest),
            }) if std::str::from_utf8(output).is_err()
                && output.len() as u64 == *output_bytes
                && output_digest(output) == *digest
        );
    }
}
