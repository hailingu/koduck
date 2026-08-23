// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! C-5 tool-call orchestration with proven-pre-effect retry (TC-08).

mod approval_resolution;
mod persistence;

use thiserror::Error;

use crate::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ApprovalRequirement, AttemptId, BindingError,
    ExactActionBinding, ExecutionAttempt, ExecutionError, TurnExecutionAuthority,
};
use crate::domain::tool::Action;
use crate::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

use super::approval_store::{ApprovalInsertResolution, ApprovalRecordStore};
use super::attempt_store::DurableAttemptTransitions;
use super::audit::{PolicyDenialContext, ToolAuditRecord, ToolAuditTrail, record_audit};
use super::execution::{
    ApprovalAuthorizer, ApprovalDecisionService, AttemptCommitter, ExecutionCoordinator,
    ExecutionPending, ExecutionPreparationError, ExecutionPreparer, IsolatedExecutor,
    LeaseValidator, ToolExecutionOutcome,
};
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::policy::{DenialCode, ToolAuthorizationService, ToolPolicyConfiguration};
use super::tool_execution_terminal::{emit_requested_approval, emit_tool_result};
use super::tool_projection::{NoToolProjections, ToolProjection, ToolProjectionSink, emit};

/// Identity and action inputs for one C-5 tool-call execution.
#[derive(Clone, Debug)]
pub struct ToolCallInputs {
    /// Tenant that owns the Turn.
    pub tenant_id: TenantId,
    /// Thread that owns the Turn.
    pub thread_id: ThreadId,
    /// Turn whose 16-slot D-7 budget this call consumes.
    pub turn_id: TurnId,
    /// Foreground lease generation that must remain current.
    pub lease_generation: LeaseGeneration,
    /// Bound Permission Profile identifier.
    pub profile_id: String,
    /// Bound Permission Profile version.
    pub profile_version: String,
    /// Owned, already-bounded model-originated action.
    pub action: Action,
    /// Absolute deadline of the owning Turn, bounding each D-6 approval window.
    pub turn_deadline_millis: u64,
}

/// A reason one C-5 tool call could not reach a terminal outcome.
#[derive(Debug, Error)]
pub enum ToolCallError {
    /// The call's tenant does not match the authenticated trust context.
    #[error("tool-call tenant does not match the authenticated principal")]
    TenantMismatch,
    /// The exact-action binding failed its shared profile-identity bound.
    #[error("tool-call binding was invalid")]
    InvalidBinding(BindingError),
    /// Default-deny policy rejected the action before D-6 or D-7 creation.
    #[error("policy denied the tool call")]
    Denied(DenialCode),
    /// The lease-validating preparer rejected the D-7 allocation.
    #[error("preparation failed before dispatch")]
    Preparation(ExecutionPreparationError),
    /// The C-7 validated approval could not be created or resolved.
    #[error("approval could not be resolved")]
    Approval(ApprovalError),
    /// No canonical terminal won; reconciliation owns the next transition.
    #[error("execution awaits canonical reconciliation")]
    Reconciliation(ExecutionPending),
}

impl ToolCallError {
    /// Returns the stable turn-level code for this failure.
    #[must_use]
    pub const fn stable_code(&self) -> &'static str {
        match self {
            Self::TenantMismatch => "TOOL_TENANT_MISMATCH",
            Self::InvalidBinding(_) => "TOOL_BINDING_INVALID",
            Self::Denied(_) => "TOOL_POLICY_DENIED",
            Self::Preparation(_) => "TOOL_PREPARATION_FAILED",
            Self::Approval(_) => "TOOL_APPROVAL_FAILED",
            Self::Reconciliation(_) => "TOOL_RECONCILIATION_REQUIRED",
        }
    }
}

/// C-5 orchestrator that drives authorize, prepare, approve, and execute with
/// exactly one proven-pre-effect retry.
///
/// Retry occurs only after a committed terminal proves
/// `effect_state = NotStarted` (TC-08). Every pass allocates a fresh D-7
/// identity, reruns descriptor/profile policy, and creates a fresh D-6 when
/// approval is required, consuming one of the Turn's 16 attempt slots per pass.
/// It calls only the lease-validating preparer and the single-dispatch
/// coordinator, so it introduces no new authority-mutation call sites.
pub(crate) struct ToolExecutionDriver<C, A> {
    policy: ToolAuthorizationService<C>,
    approval: ApprovalDecisionService<A>,
}

impl<C, A> ToolExecutionDriver<C, A> {
    /// Creates the C-5 driver around the trusted policy and C-7 approval services.
    #[must_use]
    pub(crate) const fn new(
        policy: ToolAuthorizationService<C>,
        approval: ApprovalDecisionService<A>,
    ) -> Self {
        Self { policy, approval }
    }

    /// Executes one tool call with at most one retry on a proven pre-effect failure.
    ///
    /// The call's tenant is checked against `trust`'s authenticated tenant
    /// before policy evaluation and any D-7 allocation, so a caller cannot
    /// execute or commit results under another tenant's identity, including
    /// on the approval-free `read_data` path.
    ///
    /// `decision_for` supplies the C-7 approval decision and the actual decision
    /// time for each approval-required D-6, so the D-6 expiry check uses the real
    /// approval time. `now` is a controlled clock re-read when each D-6 is
    /// created, when each prepared D-7 is durably recorded, and when each
    /// dispatch starts, and the dispatch start time is clamped to
    /// never precede the verified decision time, so a delayed approval cannot
    /// produce a D-7 start time earlier than the approval, nor a retry D-6 window
    /// computed from the original call time. The same clock is re-read by the
    /// coordinator after each executor response, so an action whose observed
    /// completion reaches the 30-second deadline commits `timed_out` instead of
    /// a succeeded or failed result. A declined, cancelled, or expired D-6
    /// cancels the prepared D-7 without dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] when policy, preparation, approval, or
    /// canonical reconciliation prevents a terminal outcome.
    pub(crate) fn execute<E, L, Co>(
        &mut self,
        preparer: &mut ExecutionPreparer<L>,
        coordinator: &mut ExecutionCoordinator<E, L, Co>,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ToolCallError>
    where
        C: ToolPolicyConfiguration,
        A: ApprovalAuthorizer,
        E: IsolatedExecutor,
        L: LeaseValidator,
        Co: AttemptCommitter + DurableAttemptTransitions,
    {
        // Convenience wrapper for callers that publish no projections and
        // configure no audit sink; the runtime's projected path carries both
        // (ADR-0003 TC-06/TC-14).
        self.execute_projected(
            preparer,
            coordinator,
            inputs,
            trust,
            decision_for,
            now,
            &mut NoToolProjections,
            &mut crate::application::NoToolAudits,
        )
    }

    /// Executes one tool call while appending D-3 projections of every
    /// canonical D-6/D-7 transition before their publication (TC-06).
    ///
    /// The projections are ordered durable views: each approval-status,
    /// dispatch, and terminal-result projection references its canonical
    /// identity and version, is appended before it is published, and can never
    /// authorize or redispatch execution. Every audit terminal is stamped by
    /// one controlled-clock read at its own emission, so a delayed approval
    /// or long-running executor never produces an observation time that
    /// predates its terminal (TC-14).
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one independently validated orchestration input"
    )]
    pub(crate) fn execute_projected<E, L, Co>(
        &mut self,
        preparer: &mut ExecutionPreparer<L>,
        coordinator: &mut ExecutionCoordinator<E, L, Co>,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        now: &mut dyn FnMut() -> u64,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
    ) -> Result<ToolExecutionOutcome, ToolCallError>
    where
        C: ToolPolicyConfiguration,
        A: ApprovalAuthorizer,
        E: IsolatedExecutor,
        L: LeaseValidator,
        Co: AttemptCommitter + DurableAttemptTransitions,
    {
        persistence::execute(
            self,
            preparer,
            coordinator,
            inputs,
            trust,
            decision_for,
            now,
            projections,
            audits,
            None,
        )
    }

    /// Executes one projected call while persisting every requested D-6
    /// through the canonical record store before its D-3 view is appended.
    #[allow(
        clippy::too_many_arguments,
        reason = "the durable D-6 port is explicit alongside the existing orchestration inputs"
    )]
    pub(crate) fn execute_projected_persisted<E, L, Co>(
        &mut self,
        preparer: &mut ExecutionPreparer<L>,
        coordinator: &mut ExecutionCoordinator<E, L, Co>,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        now: &mut dyn FnMut() -> u64,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        approval_records: &mut dyn ApprovalRecordStore,
    ) -> Result<ToolExecutionOutcome, ToolCallError>
    where
        C: ToolPolicyConfiguration,
        A: ApprovalAuthorizer,
        E: IsolatedExecutor,
        L: LeaseValidator,
        Co: AttemptCommitter + DurableAttemptTransitions,
    {
        persistence::execute(
            self,
            preparer,
            coordinator,
            inputs,
            trust,
            decision_for,
            now,
            projections,
            audits,
            Some(approval_records),
        )
    }

    /// Opens one execution pass: authorize policy, pre-validate C-7, prepare
    /// one fresh D-7, durably record it, and append its requested D-6 view
    /// only after the canonical row exists (TC-05/TC-06/TC-12).
    ///
    /// A retry that already exhausted the Turn's 16-slot budget returns
    /// [`OpenedPass::Exhausted`] with the committed `failed/attempt_limit`
    /// terminal. Every default-deny policy terminal emits one correlated,
    /// content-minimized audit record at its observation-time clock read
    /// before the typed denial propagates (TC-02/TC-14).
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one independently validated pass-opening input plus the audit, projection, and clock sinks"
    )]
    fn open_pass<E, L, Co>(
        &mut self,
        preparer: &mut ExecutionPreparer<L>,
        coordinator: &mut ExecutionCoordinator<E, L, Co>,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        approval_records: &mut Option<&mut dyn ApprovalRecordStore>,
        retried: bool,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<OpenedPass, ToolCallError>
    where
        C: ToolPolicyConfiguration,
        A: ApprovalAuthorizer,
        E: IsolatedExecutor,
        L: LeaseValidator,
        Co: AttemptCommitter + DurableAttemptTransitions,
    {
        let (mut authority, mut attempt, pre_approval) =
            match self.authorize_and_prepare(preparer, inputs, trust, now) {
                Ok((authority, attempt, pre_approval)) => (authority, attempt, pre_approval),
                Err(ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                    ExecutionError::AttemptLimit,
                ))) if retried => {
                    return Ok(OpenedPass::Exhausted(exhausted_retry_attempt_limit(
                        inputs,
                        projections,
                    )));
                }
                Err(error) => {
                    // A retry-time preparation failure (e.g., the owner was fenced)
                    // must not deliver the committed NotStarted terminal to the
                    // model; reconciliation owns the next transition.
                    if let ToolCallError::Denied(code) = error {
                        record_audit(
                            audits,
                            &ToolAuditRecord::policy_denial(&denial_context(inputs), code, now()),
                        );
                    }
                    return Err(error);
                }
            };
        // TC-12: the canonical prepared D-7 exists durably before any
        // approval resolution, dispatch, or cancellation binds to it, so
        // every later conditional transition targets the durable row and
        // a failed durable preparation fails the call closed.
        if let Err(pending) =
            coordinator.record_prepared_attempt(&mut authority, &mut attempt, now())
        {
            if retried
                && matches!(
                    &pending,
                    ExecutionPending::DispatchRejected {
                        code: ExecutionFailure::AttemptLimit
                    }
                )
            {
                return Ok(OpenedPass::Exhausted(exhausted_retry_attempt_limit(
                    inputs,
                    projections,
                )));
            }
            return Err(map_preparation_record_error(pending));
        }
        if let PreApproval::Validated(request) = &pre_approval {
            if let Some(records) = approval_records.as_deref_mut() {
                let resolution = records
                    .insert_requested(request, trust.subject_id.as_str())
                    .map_err(|_| durability_reconciliation())?;
                if !matches!(
                    resolution,
                    ApprovalInsertResolution::Inserted
                        | ApprovalInsertResolution::Existing {
                            status: crate::domain::execution::ApprovalStatus::Requested,
                            decision: None,
                            version: 1,
                        }
                ) {
                    return Err(durability_reconciliation());
                }
            }
            emit_requested_approval(request, projections);
        }
        Ok(OpenedPass::Recorded {
            authority,
            attempt: Box::new(attempt),
            pre_approval,
        })
    }

    /// Resolves one pass's approval plan: a validated D-6 is resolved — or
    /// terminalized as expired — with its correlated audit record and D-3
    /// projection, an already-expired window cancels the prepared D-7, and
    /// an approval-free action dispatches directly.
    #[allow(
        clippy::too_many_arguments,
        reason = "each parameter is one independently validated approval input plus the audit and projection sinks"
    )]
    fn resolve_plan(
        &mut self,
        pre_approval: PreApproval,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        approval_records: Option<&mut dyn ApprovalRecordStore>,
        projections: &mut dyn ToolProjectionSink,
        audits: &mut dyn ToolAuditTrail,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ApprovalPlan, ToolCallError>
    where
        A: ApprovalAuthorizer,
    {
        match pre_approval {
            PreApproval::NotRequired => Ok(ApprovalPlan::Dispatch {
                approval: None,
                earliest_start_millis: now(),
            }),
            PreApproval::AlreadyExpired => Ok(ApprovalPlan::Cancel),
            PreApproval::Validated(request) => self.resolve_validated_approval(
                *request,
                trust,
                thread_id,
                decision_for,
                approval_records,
                projections,
                audits,
                now,
            ),
        }
    }

    /// Authorizes policy, pre-validates C-7 ownership and scope, and prepares
    /// one fresh D-7 identity.
    ///
    /// For an approval-required action the C-7 check runs before the D-7
    /// allocation (TC-05): an unauthorized principal leaves no prepared
    /// attempt behind, so repeated unscoped calls cannot drain the Turn's
    /// 16-slot budget. An already-expired D-6 window still allocates and then
    /// closes the prepared D-7 without dispatch, preserving the expired-D-6
    /// cancellation contract.
    fn authorize_and_prepare<L>(
        &mut self,
        preparer: &mut ExecutionPreparer<L>,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<(TurnExecutionAuthority, ExecutionAttempt, PreApproval), ToolCallError>
    where
        C: ToolPolicyConfiguration,
        A: ApprovalAuthorizer,
        L: LeaseValidator,
    {
        let binding = ExactActionBinding::new(
            inputs.tenant_id.clone(),
            inputs.thread_id,
            inputs.turn_id,
            inputs.lease_generation,
            (inputs.profile_id.clone(), inputs.profile_version.clone()),
            AttemptId::new(),
            inputs.action.clone(),
        )
        .map_err(ToolCallError::InvalidBinding)?;
        let sealed = self
            .policy
            .authorize_binding(binding)
            .map_err(ToolCallError::Denied)?;
        let pre_approval = if matches!(
            sealed.approval_requirement(),
            Some(ApprovalRequirement::Required)
        ) {
            let requested_at_millis = now();
            match ApprovalRequest::new(
                sealed.clone(),
                requested_at_millis,
                inputs.turn_deadline_millis,
            ) {
                Ok(request) => {
                    // TC-05: the decision provider observes the D-6 only after
                    // C-7 ownership and scope validation succeeds, so an
                    // unauthorized principal can neither read the approval nor
                    // cause side effects — including a D-7 allocation.
                    self.approval
                        .validate_resolver(&request, trust, inputs.thread_id)
                        .map_err(ToolCallError::Approval)?;
                    PreApproval::Validated(Box::new(request))
                }
                Err(ApprovalError::Expired) => {
                    // TC-05/TC-09: an expired D-6 window does not waive C-7
                    // validation; without it an unscoped principal could drain
                    // the 16-slot budget through allocate-then-cancel loops.
                    self.approval
                        .validate_resolver_for_binding(&sealed, trust, inputs.thread_id)
                        .map_err(ToolCallError::Approval)?;
                    PreApproval::AlreadyExpired
                }
                Err(error) => return Err(ToolCallError::Approval(error)),
            }
        } else {
            PreApproval::NotRequired
        };
        let (authority, attempt) = preparer
            .prepare(sealed)
            .map_err(ToolCallError::Preparation)?;
        Ok((authority, attempt, pre_approval))
    }
}

/// Maps an undecidable canonical D-6 persistence result onto the existing
/// fail-closed execution reconciliation surface.
fn durability_reconciliation() -> ToolCallError {
    ToolCallError::Reconciliation(ExecutionPending::ReconciliationRequired {
        code: ExecutionFailure::DurabilityUnavailable,
        effect_state: EffectState::Unknown,
    })
}

/// Executes one resolved approval plan through the coordinator.
///
/// The dispatch start time never precedes the verified approval decision,
/// even when the clock reads earlier. The concurrent durable-claim loser
/// closed its own attempt cancelled before its rejection: that committed
/// D-7 terminal emits its correlated audit record — stamped by its own
/// clock read — before the typed reconciliation error propagates (TC-14).
fn dispatch<E, L, Co>(
    coordinator: &mut ExecutionCoordinator<E, L, Co>,
    authority: &mut TurnExecutionAuthority,
    attempt: &mut ExecutionAttempt,
    plan: ApprovalPlan,
    now: &mut dyn FnMut() -> u64,
    projections: &mut dyn ToolProjectionSink,
    audits: &mut dyn ToolAuditTrail,
) -> Result<ToolExecutionOutcome, ToolCallError>
where
    E: IsolatedExecutor,
    L: LeaseValidator,
    Co: AttemptCommitter + DurableAttemptTransitions,
{
    let executed = match plan {
        ApprovalPlan::Dispatch {
            approval,
            earliest_start_millis,
        } => {
            let started_at_millis = now().max(earliest_start_millis);
            coordinator.execute_projected(
                authority,
                approval.as_deref(),
                attempt,
                started_at_millis,
                &mut *now,
                projections,
            )
        }
        ApprovalPlan::Cancel => coordinator.cancel_prepared_attempt(authority, attempt),
    };
    match executed {
        Err(
            pending @ ExecutionPending::DispatchRejected {
                code: ExecutionFailure::ConcurrentAttempt,
            },
        ) => {
            record_audit(
                audits,
                &ToolAuditRecord::execution_terminal(
                    attempt.binding(),
                    &ToolExecutionOutcome::Cancelled {
                        effect_state: EffectState::NotStarted,
                    },
                    now(),
                ),
            );
            Err(ToolCallError::Reconciliation(pending))
        }
        Err(pending) => {
            if let Some(outcome) = committed_fenced_post_dispatch_outcome(pending, attempt) {
                // The dedicated fenced transition has already committed the
                // D-7 terminal, but it still returns reconciliation so the
                // runner does not continue the model. Its D-3 and non-atomic
                // audit views must nevertheless record that durable terminal.
                emit_tool_result(attempt, &outcome, projections);
                if !coordinator.appends_terminal_audit_atomically() {
                    record_audit(
                        audits,
                        &ToolAuditRecord::execution_terminal(attempt.binding(), &outcome, now()),
                    );
                }
            }
            Err(ToolCallError::Reconciliation(pending))
        }
        Ok(outcome) => Ok(outcome),
    }
}

/// Returns the D-7 terminal already committed by the dedicated post-dispatch
/// fence transition, when its local mirror proves this coordinator won it.
fn committed_fenced_post_dispatch_outcome(
    pending: ExecutionPending,
    attempt: &ExecutionAttempt,
) -> Option<ToolExecutionOutcome> {
    match (pending, attempt.status()) {
        (
            ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedAfterDispatch,
                effect_state,
            },
            crate::domain::execution::ExecutionStatus::Failed,
        ) => Some(ToolExecutionOutcome::Failed {
            code: ExecutionFailure::OwnerFencedAfterDispatch,
            effect_state,
        }),
        _ => None,
    }
}

/// Produces the terminal model result for a retry rejected by the exact attempt cap.
fn exhausted_retry_attempt_limit(
    inputs: &ToolCallInputs,
    projections: &mut dyn ToolProjectionSink,
) -> ToolExecutionOutcome {
    // AC-9: a retry that exhausts the 16-slot budget is failed/attempt_limit.
    emit(
        projections,
        ToolProjection::Denied {
            descriptor_id: inputs.action.descriptor_id().to_owned(),
            descriptor_version: inputs.action.descriptor_version().to_owned(),
            target: inputs.action.target().to_owned(),
            code: ExecutionFailure::AttemptLimit.stable_code().to_owned(),
        },
    );
    ToolExecutionOutcome::Failed {
        code: ExecutionFailure::AttemptLimit,
        effect_state: EffectState::NotStarted,
    }
}

/// Converts a failed durable preparation into its caller-visible C-5 outcome.
fn map_preparation_record_error(pending: ExecutionPending) -> ToolCallError {
    if matches!(
        pending,
        ExecutionPending::DispatchRejected {
            code: ExecutionFailure::AttemptLimit
        }
    ) {
        ToolCallError::Preparation(ExecutionPreparationError::Rejected(
            ExecutionError::AttemptLimit,
        ))
    } else {
        ToolCallError::Reconciliation(pending)
    }
}

/// One durably opened execution pass, or the committed terminal that ends the
/// call before approval resolution or dispatch.
enum OpenedPass {
    /// The prepared D-7 is durably recorded and its requested D-6 view is
    /// appended.
    Recorded {
        authority: TurnExecutionAuthority,
        attempt: Box<ExecutionAttempt>,
        pre_approval: PreApproval,
    },
    /// The retry already exhausted the Turn's 16-slot budget; the caller
    /// returns this committed terminal.
    Exhausted(ToolExecutionOutcome),
}

/// C-7 pre-validation outcome for one sealed binding, decided before any D-7
/// allocation.
enum PreApproval {
    /// The action needs no approval; dispatch directly.
    NotRequired,
    /// Ownership and scope validated for this exact D-6 request.
    Validated(Box<ApprovalRequest>),
    /// The D-6 window is already expired; allocate, then cancel without dispatch.
    AlreadyExpired,
}

/// Whether a prepared D-7 should dispatch with an approval or be cancelled.
///
/// `earliest_start_millis` is the verified decision time for an
/// approval-required dispatch (or the D-6 creation read otherwise); the D-7
/// start time never precedes it, by construction.
enum ApprovalPlan {
    Dispatch {
        approval: Option<Box<ApprovalRequest>>,
        earliest_start_millis: u64,
    },
    Cancel,
}

/// Builds the pre-attempt audit context for one denied call from its inputs.
///
/// The exact-action correlation digest is attempt-independent by design, so a
/// throwaway D-7 identity yields the same pre-attempt digest the sealed
/// binding would carry; an unbindable denial keeps the context without it.
fn denial_context(inputs: &ToolCallInputs) -> PolicyDenialContext {
    let mut context = PolicyDenialContext::new(
        inputs.tenant_id.clone(),
        inputs.thread_id,
        inputs.turn_id,
        inputs.lease_generation,
    )
    .with_descriptor(
        inputs.action.descriptor_id(),
        inputs.action.descriptor_version(),
    )
    .with_profile(inputs.profile_id.clone(), inputs.profile_version.clone());
    if let Ok(binding) = ExactActionBinding::new(
        inputs.tenant_id.clone(),
        inputs.thread_id,
        inputs.turn_id,
        inputs.lease_generation,
        (inputs.profile_id.clone(), inputs.profile_version.clone()),
        AttemptId::new(),
        inputs.action.clone(),
    ) {
        context = context.with_action_digest(binding.pre_attempt_audit_digest());
    }
    context
}
