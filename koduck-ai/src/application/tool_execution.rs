// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! C-5 tool-call orchestration with proven-pre-effect retry (TC-08).

use thiserror::Error;

use crate::domain::execution::{
    ApprovalDecision, ApprovalError, ApprovalRequest, ApprovalRequirement, AttemptId, BindingError,
    ExactActionBinding, ExecutionAttempt, ExecutionError, TurnExecutionAuthority,
};
use crate::domain::tool::Action;
use crate::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

use super::execution::{
    ApprovalAuthorizer, ApprovalDecisionService, AttemptCommitter, ExecutionCoordinator,
    ExecutionPending, ExecutionPreparationError, ExecutionPreparer, IsolatedExecutor,
    LeaseValidator, ToolExecutionOutcome,
};
use super::executor_envelope::{EffectState, ExecutionFailure};
use super::policy::{DenialCode, ToolAuthorizationService, ToolPolicyConfiguration};

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
    /// approval time. `now` is a controlled clock re-read when each D-6 is created
    /// and when each dispatch starts, and the dispatch start time is clamped to
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
        Co: AttemptCommitter,
    {
        if inputs.tenant_id != trust.tenant_id {
            return Err(ToolCallError::TenantMismatch);
        }
        let mut retried = false;
        loop {
            let (mut authority, mut attempt, pre_approval) =
                match self.authorize_and_prepare(preparer, inputs, trust, now) {
                    Ok((authority, attempt, pre_approval)) => (authority, attempt, pre_approval),
                    Err(ToolCallError::Preparation(ExecutionPreparationError::Rejected(
                        ExecutionError::AttemptLimit,
                    ))) if retried => {
                        // AC-9: a retry that exhausts the 16-slot budget is failed/attempt_limit.
                        return Ok(ToolExecutionOutcome::Failed {
                            code: ExecutionFailure::AttemptLimit,
                            effect_state: EffectState::NotStarted,
                        });
                    }
                    Err(error) => {
                        // A retry-time preparation failure (e.g., the owner was fenced)
                        // must not deliver the committed NotStarted terminal to the
                        // model; reconciliation owns the next transition.
                        return Err(error);
                    }
                };
            let plan = match pre_approval {
                PreApproval::NotRequired => ApprovalPlan::Dispatch {
                    approval: None,
                    earliest_start_millis: now(),
                },
                PreApproval::AlreadyExpired => ApprovalPlan::Cancel,
                PreApproval::Validated(request) => self.resolve_validated_approval(
                    *request,
                    trust,
                    inputs.thread_id,
                    decision_for,
                )?,
            };
            let executed = match plan {
                ApprovalPlan::Dispatch {
                    approval,
                    earliest_start_millis,
                } => {
                    // The dispatch start time never precedes the verified
                    // approval decision, even when the clock reads earlier.
                    let started_at_millis = now().max(earliest_start_millis);
                    coordinator.execute(
                        &mut authority,
                        approval.as_deref(),
                        &mut attempt,
                        started_at_millis,
                        &mut *now,
                    )
                }
                ApprovalPlan::Cancel => {
                    coordinator.cancel_prepared_attempt(&mut authority, &mut attempt)
                }
            };
            let outcome = match executed {
                Err(pending) => return Err(ToolCallError::Reconciliation(pending)),
                Ok(outcome) => outcome,
            };
            // Retry only on a committed executor pre-effect failure (TC-08); a
            // cancellation or success never retries even when it reports NotStarted.
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

    /// Resolves one pre-validated D-6, or signals cancellation.
    ///
    /// The D-7 is prepared before the decision is applied, so a declined,
    /// cancelled, or expired D-6 returns [`ApprovalPlan::Cancel`] to close the
    /// prepared D-7.
    fn resolve_validated_approval(
        &mut self,
        mut request: ApprovalRequest,
        trust: &TrustContext,
        thread_id: ThreadId,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
    ) -> Result<ApprovalPlan, ToolCallError>
    where
        A: ApprovalAuthorizer,
    {
        let (decision, decided_at_millis) = decision_for(&request);
        match self
            .approval
            .resolve(&mut request, trust, thread_id, decision, decided_at_millis)
        {
            Ok(_) => match decision {
                ApprovalDecision::Accepted => Ok(ApprovalPlan::Dispatch {
                    approval: Some(Box::new(request)),
                    earliest_start_millis: decided_at_millis,
                }),
                ApprovalDecision::Declined | ApprovalDecision::Cancelled => {
                    Ok(ApprovalPlan::Cancel)
                }
            },
            // A decision arriving after the D-6 expiry also cancels the prepared D-7.
            Err(ApprovalError::Expired) => Ok(ApprovalPlan::Cancel),
            Err(error) => Err(ToolCallError::Approval(error)),
        }
    }
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
