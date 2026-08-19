// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! AC-8 transport legs: authenticated C-5 interruption through the route.

use koduck_ai::application::{
    ToolInterruptionOutcome, ToolInterruptionRoute, TurnInterruptionOwnership,
    TurnOwnershipValidator,
};
use koduck_ai::domain::TrustContext;

use super::*;

fn authenticated_as(tenant: &str, subject: &str) -> TrustContext {
    TrustContext::new(TenantId::new(tenant).expect("valid tenant"), subject)
        .expect("valid principal")
}

fn authenticated(tenant: &str) -> TrustContext {
    authenticated_as(tenant, "subject-a")
}

/// Canonical-ownership double keyed to one harness: the owner subject owns
/// exactly the harness tenant, Thread, and Turn; every other identity is
/// unknown or foreign, and one switch makes canonical validation unavailable.
struct FixtureOwnership {
    tenant: TenantId,
    thread: ThreadId,
    turn: TurnId,
    unavailable: bool,
}

impl FixtureOwnership {
    fn for_harness(harness: &Harness) -> Self {
        Self {
            tenant: harness.tenant.clone(),
            thread: harness.thread,
            turn: harness.turn,
            unavailable: false,
        }
    }
}

impl TurnOwnershipValidator for FixtureOwnership {
    fn validate_turn_ownership(
        &mut self,
        trust: &TrustContext,
        thread: ThreadId,
        turn: TurnId,
    ) -> TurnInterruptionOwnership {
        if self.unavailable {
            return TurnInterruptionOwnership::Unavailable;
        }
        if trust.tenant_id == self.tenant
            && trust.subject_id == "subject-a"
            && thread == self.thread
            && turn == self.turn
        {
            TurnInterruptionOwnership::Owned
        } else {
            TurnInterruptionOwnership::UnknownOrForeign
        }
    }
}

#[test]
fn authenticated_interruption_closes_live_work_through_the_route() {
    // A pending approval plus its prepared D-7 close without any dispatch:
    // the route drives the same guarded coordinator and approval ports the
    // runtime assembles.
    let harness = Harness::new();
    let (_authority, _attempt) = harness.approval_required_prepared();
    let mut prepared_coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    let mut approvals = RecordingPendingApprovals::default();
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut prepared_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::Interrupted(
            InterruptionOutcome::Closed(ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            })
        ))
    );
    assert_eq!(prepared_coordinator.executor().dispatches, 0);
    assert_eq!(prepared_coordinator.executor().cancels, 0);
    assert_eq!(prepared_coordinator.committer().calls, 1);
    assert_eq!(
        approvals.cancelled_attempts.len(),
        1,
        "the requested D-6 was closed for the prepared D-7"
    );

    // A running attempt with an acknowledged not-started cancellation closes
    // cancelled with the exact executor-observed state.
    let harness = Harness::new();
    let (_authority, _attempt) = harness.running(1_000);
    let mut running_coordinator = coordinator(executor(CancelAcknowledgement::Acknowledged(
        CancelledEffectState::NotStarted,
    )));
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut running_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 2_000,
        ),
        Ok(ToolInterruptionOutcome::Interrupted(
            InterruptionOutcome::Closed(ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            })
        ))
    );
    // The `running` fixture claims its dispatch through the authority, so the
    // executor observes only the single bounded cancellation.
    assert_eq!(running_coordinator.executor().cancels, 1);
    assert_eq!(running_coordinator.committer().calls, 1);

    // A running attempt whose cancellation is never acknowledged reaches
    // timed_out/unknown; the closed attempt is terminal and no late executor
    // result is delivered afterwards.
    let harness = Harness::new();
    let (_authority, _attempt) = harness.running(1_000);
    let mut unacknowledged_coordinator =
        coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut unacknowledged_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut NoPendingApprovals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::Interrupted(
            InterruptionOutcome::Closed(ToolExecutionOutcome::TimedOut {
                effect_state: EffectState::Unknown,
            })
        ))
    );
    assert_eq!(unacknowledged_coordinator.executor().cancels, 1);
    assert_eq!(unacknowledged_coordinator.committer().calls, 1);
}

#[test]
fn unrouted_or_cross_tenant_interruption_is_an_indistinguishable_no_op() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.approval_required_prepared();

    // An absent Thread routing context learns nothing and mutates nothing.
    let mut no_op_coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    let mut approvals = RecordingPendingApprovals::default();
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut no_op_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            None,
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::NoLiveAttempt)
    );

    // A cross-tenant principal cannot interrupt another tenant's live work.
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut no_op_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-b"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::NoLiveAttempt)
    );
    assert_eq!(no_op_coordinator.executor().dispatches, 0);
    assert_eq!(no_op_coordinator.executor().cancels, 0);
    assert_eq!(no_op_coordinator.committer().calls, 0);
    assert!(
        approvals.cancelled_attempts.is_empty(),
        "no requested D-6 was closed for an unauthenticated or foreign interruption"
    );

    // The authenticated owner still closes the live work afterwards, proving
    // the no-op legs changed no canonical state.
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut no_op_coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::Interrupted(
            InterruptionOutcome::Closed(ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            })
        ))
    );
    // This counter belongs to the D-7 AttemptCommitter: it proves exactly one
    // attempt-terminal write, not the durable CAND-1 Turn terminal and replay
    // — no TurnHistory exists in this harness, and that AC-8 integration leg
    // remains open until the runtime composition lands.
    assert_eq!(
        no_op_coordinator.committer().calls,
        1,
        "exactly one D-7 attempt terminal is committed after the authenticated interruption"
    );
}

#[test]
fn non_owner_and_unknown_interruptions_leave_no_mutation_or_tombstone() {
    let harness = Harness::new();
    let (_authority, _attempt) = harness.approval_required_prepared();
    let mut coordinator = coordinator(executor(CancelAcknowledgement::NotAcknowledged));
    let mut approvals = RecordingPendingApprovals::default();

    // A same-tenant non-owner is indistinguishable from an unknown identity:
    // canonical ownership fails before the catalog is touched, so another
    // subject's live work is never cancelled.
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated_as("tenant-a", "subject-b"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::NoLiveAttempt)
    );

    // An unknown Turn identity is the same indistinguishable no-op.
    let unknown_turn = TurnId::new();
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            unknown_turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::NoLiveAttempt)
    );

    // A canonical-ownership outage fails closed with a typed reconciliation
    // signal instead of mutating or reporting a misleading no-op.
    let mut unavailable = FixtureOwnership::for_harness(&harness);
    unavailable.unavailable = true;
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut unavailable,
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::Unknown,
        })
    );

    assert_eq!(coordinator.executor().dispatches, 0);
    assert_eq!(coordinator.executor().cancels, 0);
    assert_eq!(coordinator.committer().calls, 0);
    assert!(
        approvals.cancelled_attempts.is_empty(),
        "no requested D-6 was closed for a rejected interruption"
    );

    // The unknown-Turn no-op retained no interruption tombstone: a fresh
    // allocation for that Turn is still admitted.
    let mut preparer = harness.runtime.preparer(AlwaysCurrentLease);
    preparer
        .prepare(sealed_binding(
            harness.tenant.clone(),
            harness.thread,
            unknown_turn,
        ))
        .expect("an unknown-identity no-op leaves no interruption tombstone");

    // The authenticated owner still closes the live work afterwards, proving
    // the rejected interruptions changed no canonical state.
    assert_eq!(
        ToolInterruptionRoute::new(&harness.runtime).interrupt(
            &mut FixtureOwnership::for_harness(&harness),
            &mut coordinator,
            &mut koduck_ai::application::NoToolAudits,
            &mut approvals,
            &authenticated("tenant-a"),
            Some(harness.thread),
            harness.turn,
            &mut || 1_000,
        ),
        Ok(ToolInterruptionOutcome::Interrupted(
            InterruptionOutcome::Closed(ToolExecutionOutcome::Cancelled {
                effect_state: EffectState::NotStarted,
            })
        ))
    );
    assert_eq!(coordinator.committer().calls, 1);
}
