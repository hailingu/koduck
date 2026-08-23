// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared projected execution loop with optional canonical D-6 persistence.

use super::{
    ApprovalAuthorizer, ApprovalDecision, ApprovalRecordStore, ApprovalRequest, AttemptCommitter,
    DurableAttemptTransitions, EffectState, ExecutionCoordinator, ExecutionPreparer,
    IsolatedExecutor, LeaseValidator, OpenedPass, ToolAuditRecord, ToolAuditTrail, ToolCallError,
    ToolCallInputs, ToolExecutionDriver, ToolExecutionOutcome, ToolPolicyConfiguration,
    ToolProjectionSink, TrustContext, denial_context, dispatch, emit_tool_result, record_audit,
};

/// Drives projected execution while carrying the canonical approval store
/// across a possible proven-pre-effect retry.
#[allow(
    clippy::too_many_arguments,
    reason = "one internal implementation serves projected test and durable production boundaries"
)]
pub(super) fn execute<C, A, E, L, Co>(
    driver: &mut ToolExecutionDriver<C, A>,
    preparer: &mut ExecutionPreparer<L>,
    coordinator: &mut ExecutionCoordinator<E, L, Co>,
    inputs: &ToolCallInputs,
    trust: &TrustContext,
    decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
    now: &mut dyn FnMut() -> u64,
    projections: &mut dyn ToolProjectionSink,
    audits: &mut dyn ToolAuditTrail,
    mut approval_records: Option<&mut dyn ApprovalRecordStore>,
) -> Result<ToolExecutionOutcome, ToolCallError>
where
    C: ToolPolicyConfiguration,
    A: ApprovalAuthorizer,
    E: IsolatedExecutor,
    L: LeaseValidator,
    Co: AttemptCommitter + DurableAttemptTransitions,
{
    if inputs.tenant_id != trust.tenant_id {
        return Err(ToolCallError::TenantMismatch);
    }
    let mut retried = false;
    loop {
        let (mut authority, mut attempt, pre_approval) = match driver.open_pass(
            preparer,
            coordinator,
            inputs,
            trust,
            &mut approval_records,
            retried,
            projections,
            audits,
            now,
        )? {
            OpenedPass::Recorded {
                authority,
                attempt,
                pre_approval,
            } => (authority, attempt, pre_approval),
            OpenedPass::Exhausted(outcome) => {
                record_audit(
                    audits,
                    &ToolAuditRecord::budget_exhausted(&denial_context(inputs), now()),
                );
                return Ok(outcome);
            }
        };
        let approval_resolution_store = approval_records
            .as_mut()
            .map(|records| &mut **records as &mut dyn ApprovalRecordStore);
        let plan = driver.resolve_plan(
            pre_approval,
            trust,
            inputs.thread_id,
            decision_for,
            approval_resolution_store,
            projections,
            audits,
            now,
        )?;
        let outcome = dispatch(
            coordinator,
            &mut authority,
            &mut attempt,
            plan,
            now,
            projections,
            audits,
        )?;
        emit_tool_result(&attempt, &outcome, projections);
        if !coordinator.appends_terminal_audit_atomically() {
            record_audit(
                audits,
                &ToolAuditRecord::execution_terminal(attempt.binding(), &outcome, now()),
            );
        }
        if matches!(
            outcome,
            ToolExecutionOutcome::Failed {
                effect_state: EffectState::NotStarted,
                ..
            }
        ) && !retried
        {
            retried = true;
            continue;
        }
        return Ok(outcome);
    }
}
