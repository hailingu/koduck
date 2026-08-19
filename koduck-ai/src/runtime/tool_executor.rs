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
    ExecutionPending, LeaseCheck, LeaseValidator, PendingApprovalCancellation,
    PendingApprovalCanceller, ToolCallError, ToolCallInputs, ToolConfigurationSnapshot,
    ToolExecutionOutcome, ToolProjection, ToolProjectionSink,
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
#[derive(Clone)]
pub(crate) struct BoundaryToolCallExecutor<C, L>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
{
    configuration: ToolConfigurationSnapshot,
    assembly: ToolExecutionAssembly,
    committer: C,
    interruption_lease: L,
}

impl<C, L> BoundaryToolCallExecutor<C, L>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
{
    /// Creates the executor over the injected authority root, snapshot,
    /// conditional terminal committer, and durable interruption-lease
    /// validator.
    pub(crate) fn new(
        root: &ToolExecutionRuntimeRoot,
        configuration: ToolConfigurationSnapshot,
        committer: C,
        interruption_lease: L,
    ) -> Self {
        Self {
            assembly: ToolExecutionAssembly::new(root, configuration.clone()),
            configuration,
            committer,
            interruption_lease,
        }
    }

    /// Records one typed policy denial without any execution, appending its
    /// D-3 view through the caller's durable projection sink (TC-02/TC-06).
    fn record_denial(
        projections: &mut dyn ToolProjectionSink,
        descriptor_id: String,
        descriptor_version: String,
        target: String,
        code: &str,
    ) -> crate::application::ModelToolResult {
        emit(
            projections,
            ToolProjection::Denied {
                descriptor_id,
                descriptor_version,
                target,
                code: code.to_owned(),
            },
        );
        crate::application::ModelToolResult {
            content: code.to_owned(),
            is_error: true,
        }
    }
}

impl<C, L> crate::application::ToolCallExecutor for BoundaryToolCallExecutor<C, L>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone + 'static,
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
        let name = if crate::domain::tool::validate_descriptor_id(&call.name).is_ok() {
            call.name
        } else {
            String::new()
        };
        let unresolved = (name.clone(), String::new(), String::new());
        let Some(descriptor) = self.configuration.descriptor_by_name(&name) else {
            let (id, version, target) = unresolved;
            return Ok(Self::record_denial(
                projections,
                id,
                version,
                target,
                DenialCode::DescriptorMissing.stable_code(),
            ));
        };
        let Some(profile) = self.configuration.first_profile() else {
            let (id, version, target) = unresolved;
            return Ok(Self::record_denial(
                projections,
                id,
                version,
                target,
                DenialCode::OutsidePermissionProfile.stable_code(),
            ));
        };
        let descriptor_id = descriptor.id().to_owned();
        let descriptor_version = descriptor.version().to_owned();
        let effect = descriptor.effect();
        let Some(target) = profile.allowed_target(&descriptor_id, &descriptor_version, effect)
        else {
            return Ok(Self::record_denial(
                projections,
                descriptor_id,
                descriptor_version,
                String::new(),
                DenialCode::OutsidePermissionProfile.stable_code(),
            ));
        };
        // Invalid arguments deny before any D-6/D-7 exists: the typed denial
        // is a recorded tool result, never a Turn terminal (ADR-0003 TC-02).
        let Ok(action) = translate_native_tool_call(
            &ConfiguredCapability::new(&descriptor_id, &descriptor_version, effect, &target),
            &call.arguments,
        ) else {
            return Ok(Self::record_denial(
                projections,
                name,
                descriptor_version,
                target,
                DenialCode::InvalidInput.stable_code(),
            ));
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
            ) {
            Ok(outcome) => outcome,
            // Every policy denial — stale, disabled, incompatible,
            // conflicting, unknown-effect, or out-of-profile descriptors, and
            // the C-5 driver's own default-deny decisions — is a recorded
            // tool result with zero D-6/D-7 and zero dispatch; only
            // non-denial failures own the Turn terminal (ADR-0003 TC-02).
            Err(ToolCallError::Denied(code)) => {
                return Ok(Self::record_denial(
                    projections,
                    descriptor_id,
                    descriptor_version,
                    target,
                    code.stable_code(),
                ));
            }
            Err(error) => return Err(error),
        };
        Ok(recorded_result(&outcome, projections))
    }
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
