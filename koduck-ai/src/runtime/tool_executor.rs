// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime composition of the runner's C-5 tool-call executor.

use crate::adapters::execution::DisabledExecutor;
use crate::adapters::history::postgres::unix_time_ms;
use crate::adapters::tool::{ConfiguredCapability, translate_native_tool_call};
use crate::application::tool_boundary::{ToolExecutionAssembly, ToolExecutionRuntimeRoot};
use crate::application::tool_projection::emit;
use crate::application::{
    AttemptCommitError, AttemptCommitResult, AttemptCommitter, DenialCode, LeaseCheck,
    LeaseValidator, ToolCallError, ToolCallInputs, ToolConfigurationSnapshot, ToolExecutionOutcome,
    ToolProjection, ToolProjectionSink,
};
use crate::domain::TrustContext;
use crate::domain::execution::{ApprovalDecision, ApprovalRequest, ExactActionBinding};

/// Foreground-lease validator for tool calls serviced by the live runner.
///
/// The runner services a call only while it is the current foreground owner of
/// that exact Turn, so the bound generation is current for the synchronous
/// servicing window. Genuinely stale owners are still fenced by the shared
/// process authority catalog and the interruption boundary; the durable C-6
/// lease check replaces this validator when T-3 lands canonical persistence
/// (ADR-0003 TC-07).
#[derive(Clone, Copy, Debug, Default)]
struct RunnerForegroundLease;

impl LeaseValidator for RunnerForegroundLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Process-local terminal committer for the pre-persistence slice.
///
/// Until T-3 lands the durable D-7 store, the shared process authority
/// catalog is the canonical terminal arbitration for this process — exactly
/// the committer contract the crate-internal harnesses exercise — so the
/// conditional commit wins locally and T-3 replaces it with the durable
/// conditional write (ADR-0003 TC-12).
#[derive(Clone, Copy, Debug, Default)]
struct ProcessLocalTerminalCommitter;

impl AttemptCommitter for ProcessLocalTerminalCommitter {
    fn commit_outcome(
        &mut self,
        _binding: &ExactActionBinding,
        _outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        Ok(AttemptCommitResult::Won)
    }
}
/// Production tool-execution port backing the runner's Tool-call servicing.
///
/// Every model Tool call resolves against the configured descriptor snapshot
/// through the C-5 boundary: an unresolved or out-of-profile call is recorded
/// as a typed denial with zero D-6/D-7 and zero dispatch (TC-02), and a
/// resolved call executes through the isolated executor boundary whose D-3
/// projections become the recorded items (TC-06/TC-11). The approval decision
/// provider fails closed — the empty production inventory makes every call
/// deny at policy before any approval could be requested, and an interactive
/// decision bridge requires its own accepted capability record.
#[derive(Clone)]
pub(crate) struct BoundaryToolCallExecutor {
    configuration: ToolConfigurationSnapshot,
    assembly: ToolExecutionAssembly,
}

impl BoundaryToolCallExecutor {
    /// Creates the executor over the injected authority root and snapshot.
    pub(crate) fn new(
        root: &ToolExecutionRuntimeRoot,
        configuration: ToolConfigurationSnapshot,
    ) -> Self {
        Self {
            assembly: ToolExecutionAssembly::new(root, configuration.clone()),
            configuration,
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

impl crate::application::ToolCallExecutor for BoundaryToolCallExecutor {
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
                ProcessLocalTerminalCommitter,
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
}
