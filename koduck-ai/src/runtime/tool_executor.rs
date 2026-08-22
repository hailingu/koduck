// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime composition of the runner's C-5 tool-call executor.

use crate::adapters::execution::DisabledExecutor;
use crate::adapters::history::postgres::{TurnTerminalObserver, unix_time_ms};
use crate::adapters::tool::{ConfiguredCapability, translate_native_tool_call};
use crate::application::tool_boundary::{ToolExecutionAssembly, ToolExecutionRuntimeRoot};
use crate::application::tool_projection::emit;
use crate::application::{
    AttemptCommitter, CanonicalTurnTerminal, DenialCode, DurableAttemptTransitions, EffectState,
    ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness, ExecutionFailure,
    ExecutionPending, InterruptionBarrierResolution, LeaseValidator, ModelToolResult,
    NoCanonicalTurnTerminal, PendingApprovalCancellation, PendingApprovalCanceller,
    PolicyDenialContext, ToolAuditRecord, ToolAuditTrail, ToolCallError, ToolCallInputs,
    ToolCallTurnContext, ToolConfigurationSnapshot, ToolExecutionOutcome, ToolProjection,
    ToolProjectionSink, record_audit,
};
use crate::domain::execution::{ApprovalDecision, ApprovalRequest, ExactActionBinding};
use crate::domain::{TenantId, ThreadId, TrustContext, TurnId};

/// Reclaims runtime-owned C-5 authority after a history-owned background
/// terminal may have committed.
///
/// Each notification clones the thread-safe canonical probe, so concurrent
/// notifications never serialize behind one lock around the bounded durable
/// probe: recovery jobs observe terminals while holding their admission
/// permits, and a shared lock would queue up to 256 two-second probes and
/// starve later recovery scheduling. A poisoned or unavailable probe leaves
/// authority retained, which is the required fail-closed outcome.
#[derive(Clone)]
pub(crate) struct AuthorityTerminalObserver<P> {
    assembly: ToolExecutionAssembly,
    terminals: P,
}

impl<P> AuthorityTerminalObserver<P>
where
    P: CanonicalTurnTerminal + Clone,
{
    /// Binds a history observer to this process's one C-5 authority root.
    pub(crate) fn new(root: &ToolExecutionRuntimeRoot, terminals: P) -> Self {
        Self {
            assembly: ToolExecutionAssembly::new(root, ToolConfigurationSnapshot::empty()),
            terminals,
        }
    }
}

impl<P> TurnTerminalObserver for AuthorityTerminalObserver<P>
where
    P: CanonicalTurnTerminal + Clone + Send + Sync,
{
    fn terminal_may_have_committed(
        &self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) {
        let mut terminals = self.terminals.clone();
        let _ = self
            .assembly
            .reclaim_terminated(&mut terminals, tenant_id, thread_id, turn_id);
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
/// bridge requires its own accepted capability record. Both the dispatch and
/// interruption paths validate the bound generation against the durable C-6
/// lease through the injected validator before any D-7 mutation, so a fenced
/// or expired owner modifies no canonical state (TC-07).
///
/// Every policy, approval, and execution terminal — including the denials
/// this executor decides before the C-5 driver exists — emits one
/// correlated, bounded audit record through the injected trail at the
/// wall-clock observation time; the production runtime wires the durable
/// `PostgreSQL` trail (TC-14).
///
/// The runner's durable Turn-terminal notification reclaims the Turn's
/// process-local authority through the injected canonical-terminal probe, so
/// a long-running process drops authority state only after its proven
/// canonical terminal and forcibly retires any cataloged live or reserved
/// work before release (ADR-0003 T-3).
#[derive(Clone)]
pub(crate) struct BoundaryToolCallExecutor<C, L, A, P = NoCanonicalTurnTerminal>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
    A: ToolAuditTrail + Clone,
    P: CanonicalTurnTerminal + Clone,
{
    configuration: ToolConfigurationSnapshot,
    assembly: ToolExecutionAssembly,
    committer: C,
    /// Injected C-6 lease validator shared by the dispatch and interruption
    /// paths: both validate the bound generation against durable lease state
    /// before any D-7 allocation, dispatch, or cancellation commits
    /// (ADR-0003 TC-07).
    lease: L,
    /// Injected audit trail receiving one correlated, bounded record per
    /// policy, approval, and execution terminal; an emission failure
    /// surfaces as a structured diagnostic without changing the committed
    /// terminal (TC-14).
    audits: A,
    /// Injected canonical Turn-terminal probe gating authority reclamation;
    /// the fail-closed default retains every Turn's process-local authority.
    terminals: P,
}

impl<C, L, A, P> BoundaryToolCallExecutor<C, L, A, P>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone,
    A: ToolAuditTrail + Clone,
    P: CanonicalTurnTerminal + Clone,
{
    /// Creates the executor over the injected authority root, snapshot,
    /// conditional terminal committer, durable C-6 lease validator, audit
    /// trail, and canonical Turn-terminal probe.
    pub(crate) fn new(
        root: &ToolExecutionRuntimeRoot,
        configuration: ToolConfigurationSnapshot,
        committer: C,
        lease: L,
        audits: A,
        terminals: P,
    ) -> Self {
        Self {
            assembly: ToolExecutionAssembly::new(root, configuration.clone()),
            configuration,
            committer,
            lease,
            audits,
            terminals,
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
        let capability =
            ConfiguredCapability::new(&descriptor_id, &descriptor_version, effect, &target);
        // Invalid provider Tool-call arguments deny before any D-6/D-7
        // exists: the typed denial is a recorded tool result, never a Turn
        // terminal (ADR-0003 TC-02). MCP has no runtime ingress yet; its
        // adapter translation remains available for its future transport.
        let Ok(action) = translate_native_tool_call(&capability, &call.arguments) else {
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

impl<C, L, A, P> crate::application::ToolCallExecutor for BoundaryToolCallExecutor<C, L, A, P>
where
    C: AttemptCommitter
        + DurableAttemptTransitions
        + ExecutionAttemptInterruptionGuard
        + ExecutionAttemptLiveness
        + Clone,
    L: LeaseValidator + Clone + 'static,
    A: ToolAuditTrail + Clone + 'static,
    P: CanonicalTurnTerminal + Clone + 'static,
{
    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<Vec<crate::application::NewItem>, ToolCallError> {
        // Commit the durable dispatch barrier before examining the local
        // catalog. Prepared insertion and dispatch claim lock and check this
        // same Turn state, so no remote instance can create work after a
        // no-live observation but before the runner writes its terminal.
        let barrier = self
            .committer
            .begin_interruption(&trust.tenant_id, thread_id, turn_id)
            .map_err(|_| {
                ToolCallError::Reconciliation(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::DurabilityUnavailable,
                    effect_state: EffectState::Unknown,
                })
            })?;
        if barrier == InterruptionBarrierResolution::NonDispatchable {
            // History won the interruption race before C-5 established its
            // barrier. Preserve history's terminal or fencing response and
            // leave local D-7 work untouched.
            return Ok(Vec::new());
        }
        let mut approvals = UnavailablePendingApprovalCanceller;
        // Both the dispatch and interruption paths validate the bound
        // generation against durable C-6 lease state before any D-7 mutation
        // commits (ADR-0003 TC-07).
        let (outcome, projections) = self
            .assembly
            .interrupt(
                DisabledExecutor,
                self.lease.clone(),
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
            Ok(false) => Ok(projections
                .into_iter()
                .flat_map(|projection| projection.d3_items())
                .collect()),
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

    fn turn_terminal_committed(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) {
        // Reclamation is hygiene bound to the durable probe: an unproven or
        // unavailable probe retains the authority, while a proven terminal
        // forcibly retires cataloged live or reserved D-7 work before release.
        // Neither outcome needs an error surface here (ADR-0003 T-3).
        let _ =
            self.assembly
                .reclaim_terminated(&mut self.terminals, tenant_id, thread_id, turn_id);
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
        // (ADR-0003 TC-06). The injected durable C-6 validator leases the
        // preparation and dispatch halves of the boundary, so a fenced or
        // expired servicing generation fails closed before any D-7
        // allocation or dispatch (ADR-0003 TC-07).
        let outcome = match self
            .assembly
            .boundary(DisabledExecutor, self.lease.clone(), self.committer.clone())
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
