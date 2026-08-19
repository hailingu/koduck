// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime composition of the runner's C-5 tool-call executor.

use crate::adapters::execution::DisabledExecutor;
use crate::adapters::history::postgres::unix_time_ms;
use crate::adapters::tool::{ConfiguredCapability, translate_native_tool_call};
use crate::application::tool_boundary::{ToolExecutionAssembly, ToolExecutionRuntimeRoot};
use crate::application::tool_projection::emit;
use crate::application::{
    AttemptCommitter, DenialCode, DurableAttemptTransitions, EffectState,
    ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness, ExecutionFailure,
    ExecutionPending, LeaseCheck, LeaseValidator, ModelToolResult, PendingApprovalCancellation,
    PendingApprovalCanceller, PolicyDenialContext, ToolAuditRecord, ToolAuditTrail, ToolCallError,
    ToolCallInputs, ToolCallTurnContext, ToolConfigurationSnapshot, ToolExecutionOutcome,
    ToolProjection, ToolProjectionSink, record_audit,
};
use crate::domain::execution::{ApprovalDecision, ApprovalRequest, ExactActionBinding};
use crate::domain::{ThreadId, TrustContext, TurnId};

/// Foreground-lease validator for tool calls serviced by the live runner.
///
/// The runner services a call only while it is the current foreground owner of
/// that exact Turn, so the bound generation is current for the synchronous
/// servicing window. Genuinely stale owners are still fenced by the shared
/// process authority catalog and the interruption boundary; the durable C-6
/// lease check replaces this validator when canonical lease persistence
/// lands (ADR-0003 TC-07). The interruption path never uses this validator:
/// an authenticated interrupt can arrive after the owning generation was
/// fenced or expired, so it validates the durable `turn_leases` row through
/// the injected [`crate::adapters::execution::SqlxTurnLeaseValidator`]
/// instead.
#[derive(Clone, Copy, Debug, Default)]
struct RunnerForegroundLease;

impl LeaseValidator for RunnerForegroundLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Fail-closed D-6 cancellation port until interactive approval transport is
/// assembled into the production runtime.
///
/// The current production capability inventory creates no approval-required
/// D-7s, so this port is never used for the configured empty inventory. If a
/// future configuration exposes such an attempt before its D-6 transport is
/// wired, interruption requires reconciliation instead of claiming that the
/// approval was cancelled.
#[derive(Clone, Copy, Debug, Default)]
struct UnavailablePendingApprovalCanceller;

impl PendingApprovalCanceller for UnavailablePendingApprovalCanceller {
    fn cancel_requested(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PendingApprovalCancellation, ExecutionPending> {
        Err(ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::Unknown,
        })
    }
}
/// Production tool-execution port backing the runner's Tool-call servicing.
///
/// Every model Tool call resolves against the configured descriptor snapshot
/// through the C-5 boundary: an unresolved or out-of-profile call is recorded
/// as a typed denial with zero D-6/D-7 and zero dispatch (TC-02), and a
/// resolved call executes through the isolated executor boundary whose D-3
/// projections become the recorded items (TC-06/TC-11). Terminal commits go
/// through the injected conditional committer — the durable
/// `SQLx` D-7 store in production (TC-12). The approval decision provider
/// fails closed — the empty production inventory makes every call deny at
/// policy before any approval could be requested, and an interactive decision
/// bridge requires its own accepted capability record. Authenticated
/// interruption validates the bound generation against the durable C-6 lease
/// through the injected interruption validator before any D-7 mutation, so a
/// fenced or expired owner modifies no canonical state (TC-07).
///
/// Every policy, approval, and execution terminal — including the denials
/// this executor decides before the C-5 driver exists — emits one
/// correlated, bounded audit record through the injected trail at the
/// wall-clock observation time; the production runtime wires the durable
/// `PostgreSQL` trail (TC-14).
#[derive(Clone)]
pub(crate) struct BoundaryToolCallExecutor<C, L, A>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
    A: ToolAuditTrail + Clone,
{
    configuration: ToolConfigurationSnapshot,
    assembly: ToolExecutionAssembly,
    committer: C,
    interruption_lease: L,
    /// Injected audit trail receiving one correlated, bounded record per
    /// policy, approval, and execution terminal; an emission failure
    /// surfaces as a structured diagnostic without changing the committed
    /// terminal (TC-14).
    audits: A,
}

impl<C, L, A> BoundaryToolCallExecutor<C, L, A>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
    A: ToolAuditTrail + Clone,
{
    /// Creates the executor over the injected authority root, snapshot,
    /// conditional terminal committer, durable interruption-lease validator,
    /// and audit trail.
    pub(crate) fn new(
        root: &ToolExecutionRuntimeRoot,
        configuration: ToolConfigurationSnapshot,
        committer: C,
        interruption_lease: L,
        audits: A,
    ) -> Self {
        Self {
            assembly: ToolExecutionAssembly::new(root, configuration.clone()),
            configuration,
            committer,
            interruption_lease,
            audits,
        }
    }

    /// Builds the pre-attempt audit context for a policy denial decided
    /// before the C-5 driver exists, from the trusted Turn identity plus
    /// whatever descriptor and Permission Profile metadata policy had
    /// already resolved (ADR-0003 TC-14).
    fn denial_audit_context(
        turn: &ToolCallTurnContext,
        descriptor: Option<(&str, &str)>,
        profile: Option<(&str, &str)>,
    ) -> PolicyDenialContext {
        let mut context = PolicyDenialContext::new(
            turn.tenant_id.clone(),
            turn.thread_id,
            turn.turn_id,
            turn.lease_generation,
        );
        if let Some((id, version)) = descriptor {
            context = context.with_descriptor(id, version);
        }
        if let Some((id, version)) = profile {
            context = context.with_profile(id, version);
        }
        context
    }

    /// Resolves one model Tool call against the configured snapshot before
    /// the C-5 driver exists: an unresolved descriptor, a missing Permission
    /// Profile, an out-of-profile target, or invalid input is a typed
    /// pre-driver denial with its correlated audit context, while a resolved
    /// call yields the owned C-5 inputs plus the descriptor view of its D-3
    /// denial path (ADR-0003 TC-02/TC-14).
    fn resolve_pre_driver(
        &self,
        call: &crate::application::ModelToolCall,
        context: &ToolCallTurnContext,
    ) -> PreDriverResolution {
        let name = if crate::domain::tool::validate_descriptor_id(&call.name).is_ok() {
            call.name.clone()
        } else {
            String::new()
        };
        let unresolved = (name.clone(), String::new(), String::new());
        let Some(descriptor) = self.configuration.descriptor_by_name(&name) else {
            return PreDriverResolution::denied(
                DenialCode::DescriptorMissing,
                Self::denial_audit_context(context, None, None),
                unresolved,
            );
        };
        let Some(profile) = self.configuration.first_profile() else {
            return PreDriverResolution::denied(
                DenialCode::OutsidePermissionProfile,
                Self::denial_audit_context(
                    context,
                    Some((descriptor.id(), descriptor.version())),
                    None,
                ),
                unresolved,
            );
        };
        let descriptor_id = descriptor.id().to_owned();
        let descriptor_version = descriptor.version().to_owned();
        let effect = descriptor.effect();
        let Some(target) = profile.allowed_target(&descriptor_id, &descriptor_version, effect)
        else {
            return PreDriverResolution::denied(
                DenialCode::OutsidePermissionProfile,
                Self::denial_audit_context(
                    context,
                    Some((descriptor_id.as_str(), descriptor_version.as_str())),
                    Some((profile.id(), profile.version())),
                ),
                (descriptor_id, descriptor_version, String::new()),
            );
        };
        // Invalid arguments deny before any D-6/D-7 exists: the typed denial
        // is a recorded tool result, never a Turn terminal (ADR-0003 TC-02).
        let Ok(action) = translate_native_tool_call(
            &ConfiguredCapability::new(&descriptor_id, &descriptor_version, effect, &target),
            &call.arguments,
        ) else {
            return PreDriverResolution::denied(
                DenialCode::InvalidInput,
                Self::denial_audit_context(
                    context,
                    Some((descriptor_id.as_str(), descriptor_version.as_str())),
                    Some((profile.id(), profile.version())),
                ),
                (name, descriptor_version, target),
            );
        };
        let inputs = ToolCallInputs {
            tenant_id: context.tenant_id.clone(),
            thread_id: context.thread_id,
            turn_id: context.turn_id,
            lease_generation: context.lease_generation,
            profile_id: profile.id().to_owned(),
            profile_version: profile.version().to_owned(),
            action,
            turn_deadline_millis: u64::MAX,
        };
        PreDriverResolution::Resolved(ResolvedCall {
            inputs,
            descriptor_id,
            descriptor_version,
            target,
        })
    }

    /// Records one typed policy denial decided before the C-5 driver exists:
    /// the correlated pre-attempt audit terminal is emitted through the
    /// configured trail at the wall-clock observation time, then the D-3
    /// denial projection and the model-bound denial result — with zero
    /// D-6/D-7 and zero dispatch (ADR-0003 TC-02/TC-06/TC-14).
    fn record_denial(
        audits: &mut dyn ToolAuditTrail,
        projections: &mut dyn ToolProjectionSink,
        audit: &PolicyDenialContext,
        denial: DenialCode,
        descriptor_id: String,
        descriptor_version: String,
        target: String,
    ) -> ModelToolResult {
        record_audit(
            audits,
            &ToolAuditRecord::policy_denial(audit, denial, unix_time_ms()),
        );
        let code = denial.stable_code();
        emit(
            projections,
            ToolProjection::Denied {
                descriptor_id,
                descriptor_version,
                target,
                code: code.to_owned(),
            },
        );
        ModelToolResult {
            content: code.to_owned(),
            is_error: true,
        }
    }

    /// Records one driver-decided typed denial: the C-5 driver already
    /// emitted its correlated audit record, so only the D-3 denial
    /// projection and the model-bound denial result remain
    /// (ADR-0003 TC-02/TC-06/TC-14).
    fn record_driver_denial(
        projections: &mut dyn ToolProjectionSink,
        descriptor_id: String,
        descriptor_version: String,
        target: String,
        code: DenialCode,
    ) -> ModelToolResult {
        let stable = code.stable_code();
        emit(
            projections,
            ToolProjection::Denied {
                descriptor_id,
                descriptor_version,
                target,
                code: stable.to_owned(),
            },
        );
        ModelToolResult {
            content: stable.to_owned(),
            is_error: true,
        }
    }
}

impl<C, L, A> crate::application::ToolCallExecutor for BoundaryToolCallExecutor<C, L, A>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone + 'static,
    A: ToolAuditTrail + Clone + 'static,
{
    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<(), ToolCallError> {
        // Commit the durable dispatch barrier before examining the local
        // catalog. Prepared insertion and dispatch claim lock and check this
        // same Turn state, so no remote instance can create work after a
        // no-live observation but before the runner writes its terminal.
        self.committer
            .begin_interruption(&trust.tenant_id, thread_id, turn_id)
            .map_err(|_| {
                ToolCallError::Reconciliation(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    effect_state: EffectState::Unknown,
                })
            })?;
        let mut approvals = UnavailablePendingApprovalCanceller;
        // The interruption lease validator answers from durable C-6 state: a
        // fenced or expired generation fails closed as reconciliation before
        // any D-7 cancellation commits (ADR-0003 TC-07).
        let outcome = self
            .assembly
            .interrupt(
                DisabledExecutor,
                self.interruption_lease.clone(),
                self.committer.clone(),
                &mut self.audits,
                &mut approvals,
                trust,
                thread_id,
                turn_id,
                &mut unix_time_ms,
            )
            .map_err(ToolCallError::Reconciliation)?;
        interruption_outcome_result(&outcome)?;
        match self
            .committer
            .has_live_attempt(&trust.tenant_id, thread_id, turn_id)
        {
            Ok(false) => Ok(()),
            // The process-local catalog cannot cancel or reconcile durable
            // work owned by another instance. A local close is not sufficient:
            // do not let the runner write a Turn terminal until every durable
            // D-7 is closed or reconciled through the canonical path.
            Ok(true) => Err(ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    effect_state: EffectState::Unknown,
                },
            )),
            Err(_) => Err(ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    effect_state: EffectState::Unknown,
                },
            )),
        }
    }

    fn execute_tool_call(
        &mut self,
        call: crate::application::ModelToolCall,
        context: &crate::application::ToolCallTurnContext,
        trust: &TrustContext,
        projections: &mut dyn ToolProjectionSink,
    ) -> Result<crate::application::ModelToolResult, ToolCallError> {
        let (inputs, descriptor_id, descriptor_version, target) =
            match self.resolve_pre_driver(&call, context) {
                PreDriverResolution::Denied(denial) => {
                    return Ok(Self::record_denial(
                        &mut self.audits,
                        projections,
                        &denial.audit,
                        denial.code,
                        denial.descriptor_id,
                        denial.descriptor_version,
                        denial.target,
                    ));
                }
                PreDriverResolution::Resolved(ResolvedCall {
                    inputs,
                    descriptor_id,
                    descriptor_version,
                    target,
                }) => (inputs, descriptor_id, descriptor_version, target),
            };
        let mut decision = |_request: &ApprovalRequest| (ApprovalDecision::Cancelled, 0);
        // The runner-supplied sink is forwarded through the boundary, so the
        // approval, dispatch, and terminal projections are durably appended
        // as they happen — the running view before the executor dispatch
        // (ADR-0003 TC-06).
        let outcome = match self
            .assembly
            .boundary(
                DisabledExecutor,
                RunnerForegroundLease,
                self.committer.clone(),
            )
            .execute_projected(
                &inputs,
                trust,
                &mut decision,
                &mut unix_time_ms,
                projections,
                &mut self.audits,
            ) {
            Ok(outcome) => outcome,
            // Every policy denial — stale, disabled, incompatible,
            // conflicting, unknown-effect, or out-of-profile descriptors, and
            // the C-5 driver's own default-deny decisions — is a recorded
            // tool result with zero D-6/D-7 and zero dispatch; only
            // non-denial failures own the Turn terminal (ADR-0003 TC-02).
            // The driver already emitted this denial's correlated audit
            // record (TC-14).
            Err(ToolCallError::Denied(code)) => {
                return Ok(Self::record_driver_denial(
                    projections,
                    descriptor_id,
                    descriptor_version,
                    target,
                    code,
                ));
            }
            Err(error) => return Err(error),
        };
        Ok(recorded_result(&outcome, projections))
    }
}

/// One pre-driver policy decision for a model Tool call, made before any
/// C-5 driver, D-6, or D-7 exists.
enum PreDriverResolution {
    /// The call resolved to a configured capability, bound Permission
    /// Profile, and owned action, ready for the C-5 driver.
    Resolved(ResolvedCall),
    /// Policy denied the call with its correlated audit context and D-3
    /// denial view.
    Denied(PreDriverDenial),
}

impl PreDriverResolution {
    /// Builds one pre-driver denial from its typed code, correlated audit
    /// context, and (`descriptor_id`, `descriptor_version`, `target`) view.
    fn denied(
        code: DenialCode,
        audit: PolicyDenialContext,
        view: (String, String, String),
    ) -> Self {
        let (descriptor_id, descriptor_version, target) = view;
        Self::Denied(PreDriverDenial {
            code,
            audit,
            descriptor_id,
            descriptor_version,
            target,
        })
    }
}

/// The resolved pre-driver view of one model Tool call.
struct ResolvedCall {
    inputs: ToolCallInputs,
    descriptor_id: String,
    descriptor_version: String,
    target: String,
}

/// One typed policy denial decided before the C-5 driver exists.
struct PreDriverDenial {
    code: DenialCode,
    audit: PolicyDenialContext,
    descriptor_id: String,
    descriptor_version: String,
    target: String,
}

/// Converts an interruption result into the runner contract without hiding a
/// partially closed Turn whose remaining D-7s still need reconciliation.
fn interruption_outcome_result(
    outcome: &crate::application::InterruptionOutcome,
) -> Result<(), ToolCallError> {
    match outcome {
        crate::application::InterruptionOutcome::NoLiveAttempt
        | crate::application::InterruptionOutcome::Closed(_)
        | crate::application::InterruptionOutcome::ClosedMany(_) => Ok(()),
        crate::application::InterruptionOutcome::PartiallyClosed { pending, .. } => {
            Err(ToolCallError::Reconciliation(*pending))
        }
    }
}

/// Maps one committed C-5 outcome onto the bounded model-bound continuation
/// result.
///
/// The D-3 items were appended durably through the forwarded runner sink as
/// the projections happened; the continuation carries the committed output,
/// or a stable failure summary when the call produced none or its opaque
/// output is not valid UTF-8. The driver's return already proves the
/// current-generation durable commit (ADR-0003 TC-11).
fn recorded_result(
    outcome: &ToolExecutionOutcome,
    projections: &mut dyn ToolProjectionSink,
) -> crate::application::ModelToolResult {
    match outcome {
        ToolExecutionOutcome::Succeeded { output, .. } => {
            if let Ok(text) = std::str::from_utf8(output) {
                crate::application::ModelToolResult {
                    content: text.to_owned(),
                    is_error: false,
                }
            } else {
                // Committed output stays opaque bytes; a non-UTF-8 result is
                // rejected at the model boundary with a stable summary rather
                // than lossy expansion or byte corruption (ADR-0003 TC-09/TC-11).
                projections.bind_opaque_success_summary(output);
                crate::application::ModelToolResult {
                    content: "output_invalid_utf8".to_owned(),
                    is_error: true,
                }
            }
        }
        ToolExecutionOutcome::Failed { code, .. } => crate::application::ModelToolResult {
            content: code.stable_code().to_owned(),
            is_error: true,
        },
        ToolExecutionOutcome::TimedOut { .. } => crate::application::ModelToolResult {
            content: "timed_out".to_owned(),
            is_error: true,
        },
        ToolExecutionOutcome::Cancelled { .. } => crate::application::ModelToolResult {
            content: "cancelled".to_owned(),
            is_error: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::EffectState;

    /// Projection sink that records the opaque bytes bound to a model summary.
    #[derive(Default)]
    struct SummaryBindingRecorder {
        bound_output: Option<Vec<u8>>,
    }

    impl ToolProjectionSink for SummaryBindingRecorder {
        fn append(
            &mut self,
            _projection: &ToolProjection,
        ) -> Result<(), crate::application::ToolProjectionError> {
            Ok(())
        }

        fn publish(&mut self, _projection: &ToolProjection) {}

        fn bind_opaque_success_summary(&mut self, output: &[u8]) {
            self.bound_output = Some(output.to_vec());
        }
    }

    fn succeeded_outcome(output: Vec<u8>) -> ToolExecutionOutcome {
        ToolExecutionOutcome::Succeeded {
            output,
            effect_state: EffectState::Started,
        }
    }

    #[test]
    fn non_utf8_committed_output_is_rejected_without_lossy_expansion() {
        // Lossy conversion would expand each invalid byte to the 3-byte
        // replacement character while corrupting the opaque committed bytes;
        // the model-bound view rejects non-UTF-8 output with a stable summary
        // instead, while the durable tool_result projection recorded through
        // the sink still reports the exact committed byte count
        // (ADR-0003 TC-09/TC-11).
        let output = vec![0xff, 0xfe, 0xfd];
        let mut projections = SummaryBindingRecorder::default();
        let result = recorded_result(&succeeded_outcome(output.clone()), &mut projections);

        assert!(result.is_error);
        assert_eq!(result.content, "output_invalid_utf8");
        assert_eq!(projections.bound_output, Some(output));
    }

    #[test]
    fn utf8_committed_output_reaches_the_model_unchanged() {
        let mut projections = SummaryBindingRecorder::default();
        let result = recorded_result(
            &succeeded_outcome("valid é output".as_bytes().to_vec()),
            &mut projections,
        );

        assert!(!result.is_error);
        assert_eq!(result.content, "valid é output");
        assert_eq!(projections.bound_output, None);
    }

    #[test]
    fn partially_closed_interruption_requires_reconciliation() {
        let pending = ExecutionPending::ReconciliationRequired {
            code: ExecutionFailure::DurabilityUnavailable,
            effect_state: EffectState::Unknown,
        };
        let result = interruption_outcome_result(
            &crate::application::InterruptionOutcome::PartiallyClosed {
                closed: vec![ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                }],
                pending,
            },
        );

        assert!(matches!(
            result,
            Err(ToolCallError::Reconciliation(
                ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    effect_state: EffectState::Unknown,
                }
            ))
        ));
    }
}
