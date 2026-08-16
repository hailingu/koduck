// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authenticated C-5 interruption route over the guarded cancellation path.

use crate::domain::{ThreadId, TrustContext, TurnId};

use super::cancellation::{
    AttemptCancellationService, ExecutionInterrupter, InterruptionOutcome, PendingApprovalCanceller,
};
use super::execution::ExecutionPending;
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::preparation::ToolExecutionRuntime;

/// Typed canonical subject-ownership answer for one identified Turn.
///
/// An unknown identity and a foreign owner are one indistinguishable answer,
/// so the interruption route cannot be used to probe live-work existence
/// (ADR-0003 TC-05).
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the runtime interrupt composition lands with the T-3 durable D-7 committer that supplies its cancellation ports"
    )
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TurnInterruptionOwnership {
    /// The authenticated subject canonically owns this tenant, Thread, and Turn.
    Owned,
    /// The identity is unknown to canonical history or owned by another
    /// subject; the two cases are deliberately indistinguishable.
    UnknownOrForeign,
    /// Canonical ownership could not be validated; the route fails closed
    /// without touching the authority catalog.
    Unavailable,
}

/// Consumer-owned port for canonical subject ownership of one Turn.
///
/// The canonical history adapter owns the durable tenant/Thread/Turn and
/// subject relationship; the route MUST consult it before touching the
/// authority catalog, so a same-tenant non-owner or an unknown identity can
/// neither cancel another subject's work nor leave an interruption tombstone
/// behind.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the runtime interrupt composition lands with the T-3 durable D-7 committer that supplies its cancellation ports"
    )
)]
pub(crate) trait TurnOwnershipValidator {
    /// Reports the typed canonical ownership of the identified Turn.
    fn validate_turn_ownership(
        &mut self,
        trust: &TrustContext,
        thread: ThreadId,
        turn: TurnId,
    ) -> TurnInterruptionOwnership;
}

/// The route-level outcome of one authenticated C-5 interruption.
///
/// An absent Thread routing context, an unknown Turn, and a cross-tenant
/// principal are one indistinguishable [`ToolInterruptionOutcome::NoLiveAttempt`]:
/// the route mutates no state and exposes no live-work existence (ADR-0003
/// TC-05/TC-10).
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the runtime interrupt composition lands with the T-3 durable D-7 committer that supplies its cancellation ports"
    )
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ToolInterruptionOutcome {
    /// The authenticated principal's interruption reached the guarded C-5
    /// cancellation path and produced this truthful interruption outcome.
    Interrupted(InterruptionOutcome),
    /// No live work was interrupted for this principal, Thread, and Turn.
    NoLiveAttempt,
}

/// Authenticated interruption route sharing the runtime's C-5 authority.
///
/// The route derives the tenant only from the gateway-validated trust context
/// and the Thread only from validated routing context, then drives the same
/// guarded coordinator and approval ports the runtime assembles; it introduces
/// no new authority-mutation call site (ADR-0003 TC-10).
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the runtime interrupt composition lands with the T-3 durable D-7 committer that supplies its cancellation ports"
    )
)]
pub(crate) struct ToolInterruptionRoute {
    interrupter: ExecutionInterrupter,
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the runtime interrupt composition lands with the T-3 durable D-7 committer that supplies its cancellation ports"
    )
)]
impl ToolInterruptionRoute {
    /// Creates the route from the runtime's shared authority catalog.
    pub(crate) fn new(runtime: &ToolExecutionRuntime) -> Self {
        Self {
            interrupter: runtime.interrupter(),
        }
    }

    /// Applies one authenticated interruption for the identified Turn.
    ///
    /// `thread` is the Thread routing context the presentation server
    /// validated as a well-formed identity. An absent context fails closed as
    /// [`ToolInterruptionOutcome::NoLiveAttempt`] with zero service calls, so
    /// a caller cannot probe live-work existence through the interruption
    /// route.
    ///
    /// Canonical subject ownership is validated before the authority catalog
    /// is touched: an unknown identity or a same-tenant non-owner is the same
    /// indistinguishable [`ToolInterruptionOutcome::NoLiveAttempt`] with zero
    /// catalog access, so it neither cancels another subject's work nor
    /// leaves an interruption tombstone behind.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when canonical ownership could not be
    /// validated or no canonical terminal write won; the reconciler owns the
    /// next transition and no terminal is fabricated.
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one independently validated orchestration input"
    )]
    pub(crate) fn interrupt(
        &self,
        ownership: &mut dyn TurnOwnershipValidator,
        cancellations: &mut dyn AttemptCancellationService,
        approvals: &mut dyn PendingApprovalCanceller,
        trust: &TrustContext,
        thread: Option<ThreadId>,
        turn: TurnId,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolInterruptionOutcome, ExecutionPending> {
        let Some(thread) = thread else {
            return Ok(ToolInterruptionOutcome::NoLiveAttempt);
        };
        match ownership.validate_turn_ownership(trust, thread, turn) {
            TurnInterruptionOwnership::UnknownOrForeign => {
                return Ok(ToolInterruptionOutcome::NoLiveAttempt);
            }
            TurnInterruptionOwnership::Unavailable => {
                return Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    effect_state: EffectState::Unknown,
                });
            }
            TurnInterruptionOwnership::Owned => {}
        }
        match self.interrupter.interrupt(
            cancellations,
            approvals,
            &trust.tenant_id,
            thread,
            turn,
            now,
        ) {
            Ok(InterruptionOutcome::NoLiveAttempt) => Ok(ToolInterruptionOutcome::NoLiveAttempt),
            Ok(interrupted) => Ok(ToolInterruptionOutcome::Interrupted(interrupted)),
            Err(pending) => Err(pending),
        }
    }
}
