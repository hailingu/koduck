// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Canonical and process-local D-6 resolution for one prepared execution pass.

use crate::domain::execution::{ApprovalDecision, ApprovalRequest, ApprovalStatus, ApproverId};
use crate::domain::{ThreadId, TrustContext};

use crate::application::ApprovalDecisionResolution;

use super::{
    ApprovalAuthorizer, ApprovalPlan, ApprovalRecordStore, ToolAuditRecord, ToolAuditTrail,
    ToolCallError, ToolExecutionDriver, ToolProjection, ToolProjectionSink,
    durability_reconciliation, emit, record_audit,
};

impl<C, A> ToolExecutionDriver<C, A>
where
    A: ApprovalAuthorizer,
{
    /// Resolves one pre-validated D-6 through the configured canonical store,
    /// retaining the process-local state machine only for non-persisted tests.
    #[allow(
        clippy::too_many_arguments,
        reason = "the resolution carries one authenticated decision plus its projection, audit, and clock boundaries"
    )]
    pub(super) fn resolve_validated_approval(
        &mut self,
        request: ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        approval_records: Option<&mut dyn ApprovalRecordStore>,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ApprovalPlan, ToolCallError> {
        let (decision, decided_at_millis) = decision_for(&request);
        if let Some(records) = approval_records {
            return self.resolve_persisted_approval(
                request,
                trust,
                thread_id,
                decision,
                decided_at_millis,
                records,
                projections,
                audits,
                now,
            );
        }
        self.resolve_local_approval(
            request,
            trust,
            thread_id,
            decision,
            decided_at_millis,
            projections,
            audits,
            now,
        )
    }

    /// Resolves and projects one D-6 strictly from the canonical store result.
    #[allow(
        clippy::too_many_arguments,
        reason = "the canonical transition carries its authenticated identity and observation sinks explicitly"
    )]
    fn resolve_persisted_approval(
        &mut self,
        mut request: ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
        records: &mut dyn ApprovalRecordStore,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ApprovalPlan, ToolCallError> {
        let audit_owned_atomically = records.appends_terminal_audit_atomically();
        let approver = ApproverId::from_authenticated(trust).ok_or(ToolCallError::Approval(
            crate::domain::execution::ApprovalError::NotAuthorized,
        ))?;
        let resolution = records
            .resolve_decision(
                request.approval_id(),
                request.tenant_id(),
                thread_id,
                trust.subject_id.as_str(),
                decision,
                &approver,
                decided_at_millis,
            )
            .map_err(|_| durability_reconciliation())?;
        let (status, canonical_decision, version, won) = match resolution {
            ApprovalDecisionResolution::Won { decision, version } => (
                ApprovalStatus::from_decision(decision),
                Some(decision),
                version,
                true,
            ),
            ApprovalDecisionResolution::ExistingTerminal {
                decision,
                status,
                version,
            } if canonical_terminal_is_valid(status, decision) => {
                (status, decision, version, false)
            }
            ApprovalDecisionResolution::TurnGuardRejected
            | ApprovalDecisionResolution::NotFound
            | ApprovalDecisionResolution::ExistingTerminal { .. } => {
                return Err(durability_reconciliation());
            }
        };

        let earliest_start_millis = if status == ApprovalStatus::Accepted {
            // The canonical store owns the transition. This local mutation is
            // only the exact-binding capability consumed by the in-process
            // dispatch guard; it is never projected or persisted as authority.
            let mirror_time = if won {
                decided_at_millis
            } else {
                request.requested_at_millis()
            };
            self.approval
                .resolve(
                    &mut request,
                    trust,
                    thread_id,
                    ApprovalDecision::Accepted,
                    mirror_time,
                )
                .map_err(ToolCallError::Approval)?;
            Some(if won { decided_at_millis } else { now() })
        } else {
            None
        };
        emit_terminal(
            &request,
            status,
            canonical_decision,
            version,
            audit_owned_atomically,
            projections,
            audits,
            now,
        );
        Ok(match earliest_start_millis {
            Some(earliest_start_millis) => ApprovalPlan::Dispatch {
                approval: Some(Box::new(request)),
                earliest_start_millis,
            },
            None => ApprovalPlan::Cancel,
        })
    }

    /// Preserves the original in-process state machine for callers without a
    /// durable D-6 store, while emitting the same terminal view contract.
    #[allow(
        clippy::too_many_arguments,
        reason = "the local fallback carries its authenticated decision and observation sinks explicitly"
    )]
    fn resolve_local_approval(
        &mut self,
        mut request: ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision: ApprovalDecision,
        decided_at_millis: u64,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ApprovalPlan, ToolCallError> {
        match self
            .approval
            .resolve(&mut request, trust, thread_id, decision, decided_at_millis)
        {
            Ok(_) => {
                emit_terminal(
                    &request,
                    request.status(),
                    request.decision(),
                    request.version(),
                    false,
                    projections,
                    audits,
                    now,
                );
                Ok(match decision {
                    ApprovalDecision::Accepted => ApprovalPlan::Dispatch {
                        approval: Some(Box::new(request)),
                        earliest_start_millis: decided_at_millis,
                    },
                    ApprovalDecision::Declined | ApprovalDecision::Cancelled => {
                        ApprovalPlan::Cancel
                    }
                })
            }
            Err(crate::domain::execution::ApprovalError::Expired) => {
                emit_terminal(
                    &request,
                    request.status(),
                    request.decision(),
                    request.version(),
                    false,
                    projections,
                    audits,
                    now,
                );
                Ok(ApprovalPlan::Cancel)
            }
            Err(error) => Err(ToolCallError::Approval(error)),
        }
    }
}

/// Emits one terminal audit and D-3 view from the supplied canonical fields.
#[allow(
    clippy::too_many_arguments,
    reason = "the canonical projection fields and its two observation sinks are explicit"
)]
fn emit_terminal(
    request: &ApprovalRequest,
    status: ApprovalStatus,
    decision: Option<ApprovalDecision>,
    version: u64,
    audit_owned_atomically: bool,
    projections: &mut dyn ToolProjectionSink,
    audits: &mut dyn ToolAuditTrail,
    now: &mut dyn FnMut() -> u64,
) {
    if !audit_owned_atomically {
        record_audit(
            audits,
            &ToolAuditRecord::approval_resolution(
                request.binding(),
                request.approval_id(),
                status,
                decision,
                version,
                now(),
            ),
        );
    }
    emit(
        projections,
        ToolProjection::ApprovalStatus {
            approval_id: request.approval_id(),
            attempt_id: request.binding().attempt_id(),
            status,
            decision,
            version,
        },
    );
}

/// Validates the complete immutable D-6 terminal tuple returned by a replay.
fn canonical_terminal_is_valid(status: ApprovalStatus, decision: Option<ApprovalDecision>) -> bool {
    match status {
        ApprovalStatus::Accepted => decision == Some(ApprovalDecision::Accepted),
        ApprovalStatus::Declined => decision == Some(ApprovalDecision::Declined),
        ApprovalStatus::Cancelled => decision == Some(ApprovalDecision::Cancelled),
        ApprovalStatus::Expired => decision.is_none(),
        ApprovalStatus::Requested => false,
    }
}
