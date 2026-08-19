// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Public C-5 tool-execution boundary assembled from trusted crate services.

use std::sync::{Arc, Mutex};

use crate::domain::execution::{ApprovalDecision, ApprovalRequest, ExactActionBinding};
use crate::domain::{ThreadId, TrustContext};

use super::attempt_store::DurableAttemptTransitions;
use super::cancellation::{ExecutionInterrupter, InterruptionOutcome, PendingApprovalCanceller};
use super::execution::{
    ApprovalAuthorizer, ApprovalDecisionService, AttemptCommitter, ExecutionCoordinator,
    ExecutionPending, ExecutionPreparer, IsolatedExecutor, LeaseCheck, LeaseValidator,
    ToolExecutionAuthorityRoot, ToolExecutionOutcome, ToolExecutionRuntime,
};
use super::policy::{TOOL_APPROVAL_SCOPE, ToolAuthorizationService, ToolConfigurationSnapshot};
use super::tool_execution::{ToolCallError, ToolCallInputs, ToolExecutionDriver};

/// Lease validator shared by the preparation and commit halves of one
/// boundary, so both observe the same C-6 generation check.
#[derive(Clone)]
struct SharedLeaseValidator(Arc<Mutex<dyn LeaseValidator>>);

impl LeaseValidator for SharedLeaseValidator {
    fn check_current(&mut self, binding: &ExactActionBinding) -> LeaseCheck {
        // A panicking validator may have left partially updated state behind
        // the poisoned lock; ownership is undetermined, so every later check
        // reports Unavailable instead of guessing or reusing that state
        // (TC-07). The typed outcome is the machine-readable diagnostic that
        // surfaces as reconciliation; a structured log sink requires a logging
        // dependency outside this slice's authorized scope and lands with the
        // T-2 runtime observability wiring.
        match self.0.lock() {
            Ok(mut lease) => lease.check_current(binding),
            Err(poisoned) => {
                drop(poisoned);
                LeaseCheck::Unavailable
            }
        }
    }
}

/// Crate-owned C-7 scope check: only a principal carrying the validated
/// `ai.tool.approve` scope may resolve a requested D-6 (TC-05).
struct ToolApprovalScopeAuthorizer;

impl ApprovalAuthorizer for ToolApprovalScopeAuthorizer {
    fn can_resolve_tool_approval(
        &mut self,
        _binding: &ExactActionBinding,
        trust: &TrustContext,
        _thread_id: ThreadId,
    ) -> bool {
        trust.has_approval_scope(TOOL_APPROVAL_SCOPE)
    }
}

/// Explicitly owned C-5 Turn authority root that the runtime injects into
/// every [`ToolExecutionAssembly`] (TC-09/TC-12).
///
/// [`Self::issue`] is the controlled factory: production runtime assembly
/// issues exactly one root, holds it explicitly, and injects it wherever
/// assemblies are created, so one Turn keeps exactly one 16-slot attempt
/// budget and one running D-7. There is deliberately no global root and no
/// hidden issuance path — two independently issued roots are two separate
/// authority spaces, exactly like two processes, and only the hosting
/// runtime decides how many exist.
#[derive(Clone, Debug)]
pub(crate) struct ToolExecutionRuntimeRoot {
    runtime: ToolExecutionRuntime,
}

impl ToolExecutionRuntimeRoot {
    /// Issues the runtime-owned C-5 Turn authority root.
    ///
    /// Issuance is crate-internal and runtime assembly is its sole call site;
    /// every other component receives shared root handles, so no caller can
    /// mint a second authority root for the same process.
    pub(crate) fn issue() -> Self {
        Self {
            runtime: ToolExecutionRuntime::new(&ToolExecutionAuthorityRoot::new()),
        }
    }

    /// Returns the shared runtime only for crate-internal composition tests.
    #[cfg(test)]
    pub(crate) fn runtime(&self) -> ToolExecutionRuntime {
        self.runtime.clone()
    }
}

/// Runtime assembly whose boundaries share one explicitly injected C-5 Turn
/// authority root (TC-09/TC-12).
///
/// Every boundary derived from one assembly — and every assembly bound to the
/// same injected [`ToolExecutionRuntimeRoot`] — shares one authority catalog,
/// so one Turn has exactly one 16-slot attempt budget and one running D-7.
#[derive(Clone)]
pub(crate) struct ToolExecutionAssembly {
    configuration: ToolConfigurationSnapshot,
    runtime: ToolExecutionRuntime,
}

impl ToolExecutionAssembly {
    /// Creates one assembly bound to the explicitly injected authority root.
    ///
    /// The single `lease` validator of each derived boundary is shared by
    /// preparation and commit, so both halves of every D-7 observe the same
    /// C-6 foreground-generation check.
    #[must_use]
    pub(crate) fn new(
        root: &ToolExecutionRuntimeRoot,
        configuration: ToolConfigurationSnapshot,
    ) -> Self {
        Self {
            configuration,
            runtime: root.runtime.clone(),
        }
    }

    /// Creates one port-specific boundary sharing this assembly's injected
    /// authority root, crate-owned sealing service, and scope-checked
    /// approval service.
    ///
    /// The supplied committer object is also the durable canonical D-7
    /// authority: every derived boundary records its prepared D-7s and claims
    /// the single durable running slot through the same store that commits
    /// its terminals, so no composition can dispatch around the durable
    /// transitions (TC-12).
    pub(crate) fn boundary<E, L, C>(
        &self,
        executor: E,
        lease: L,
        committer: C,
    ) -> ToolExecutionBoundary<E, C>
    where
        E: IsolatedExecutor,
        L: LeaseValidator + 'static,
        C: AttemptCommitter + DurableAttemptTransitions,
    {
        let shared_lease = SharedLeaseValidator(Arc::new(Mutex::new(lease)));
        ToolExecutionBoundary {
            driver: ToolExecutionDriver::new(
                ToolAuthorizationService::new(self.configuration.clone()),
                ApprovalDecisionService::new(ToolApprovalScopeAuthorizer),
            ),
            preparer: self
                .runtime
                .preparer(SharedLeaseValidator(Arc::clone(&shared_lease.0))),
            coordinator: ExecutionCoordinator::new(executor, shared_lease, committer),
        }
    }

    /// Cancels every live D-7 for an authenticated Turn through the shared
    /// runtime authority catalog.
    ///
    /// The interruption coordinator is assembled from the same executor,
    /// lease validator, and conditional terminal committer as normal dispatch,
    /// so cancellation cannot bypass C-5 ownership or durable-terminal rules.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionPending`] when a live D-7 needs reconciliation.
    #[allow(
        clippy::too_many_arguments,
        reason = "the C-5 cancellation ports and authenticated ownership dimensions are explicit"
    )]
    pub(crate) fn interrupt<E, L, C, A>(
        &self,
        executor: E,
        lease: L,
        committer: C,
        approvals: &mut A,
        trust: &TrustContext,
        thread_id: ThreadId,
        turn_id: crate::domain::TurnId,
        now: &mut dyn FnMut() -> u64,
    ) -> Result<InterruptionOutcome, ExecutionPending>
    where
        E: IsolatedExecutor,
        L: LeaseValidator + 'static,
        C: AttemptCommitter + DurableAttemptTransitions,
        A: PendingApprovalCanceller,
    {
        let shared_lease = SharedLeaseValidator(Arc::new(Mutex::new(lease)));
        let mut coordinator = ExecutionCoordinator::new(executor, shared_lease, committer);
        ExecutionInterrupter::interrupt(
            &self.runtime.interrupter(),
            &mut coordinator,
            approvals,
            &trust.tenant_id,
            thread_id,
            turn_id,
            now,
        )
    }
}

/// Public entry to the C-5 default-deny policy and isolated executor boundary.
///
/// Boundaries are created only by [`ToolExecutionAssembly::boundary`], so
/// callers supply only consumer-owned ports and validated configuration
/// values — never an authority issuer, a self-asserted authorizer, or a
/// caller-constructed authority store. Binding sealing remains crate-internal
/// (TC-01, TC-03, TC-05) and every boundary sharing an injected root shares
/// its single Turn authority space (TC-09, TC-12).
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime execution wiring is not complete")
)]
pub(crate) struct ToolExecutionBoundary<E, C> {
    driver: ToolExecutionDriver<ToolConfigurationSnapshot, ToolApprovalScopeAuthorizer>,
    preparer: ExecutionPreparer<SharedLeaseValidator>,
    coordinator: ExecutionCoordinator<E, SharedLeaseValidator, C>,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "T-2 runtime execution wiring is not complete")
)]
impl<E, C> ToolExecutionBoundary<E, C>
where
    E: IsolatedExecutor,
    C: AttemptCommitter + DurableAttemptTransitions,
{
    /// Executes one tool call with the approved retry, bounds, and fencing
    /// contract of ADR-0003.
    ///
    /// The call's tenant must match `trust`'s authenticated tenant before any
    /// policy evaluation or D-7 allocation, so a caller cannot execute or
    /// commit results under another tenant's identity, including on the
    /// approval-free `read_data` path. `decision_for` supplies the approval
    /// decision and its actual decision time for each approval-required D-6;
    /// the decision is still validated against `trust` for tenant, Thread,
    /// and `ai.tool.approve` scope before it can authorize the exact attempt.
    /// `now` supplies the controlled C-5 clock.
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] when identity, policy, preparation, approval,
    /// or canonical reconciliation prevents a terminal outcome.
    pub(crate) fn execute(
        &mut self,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        now: &mut dyn FnMut() -> u64,
    ) -> Result<ToolExecutionOutcome, ToolCallError> {
        self.driver.execute(
            &mut self.preparer,
            &mut self.coordinator,
            inputs,
            trust,
            decision_for,
            now,
        )
    }

    /// Executes one call while appending D-3 projections of every canonical
    /// D-6/D-7 transition before their publication (TC-06).
    ///
    /// # Errors
    ///
    /// Returns [`ToolCallError`] under the same conditions as [`Self::execute`].
    pub(crate) fn execute_projected(
        &mut self,
        inputs: &ToolCallInputs,
        trust: &TrustContext,
        decision_for: &mut dyn FnMut(&ApprovalRequest) -> (ApprovalDecision, u64),
        now: &mut dyn FnMut() -> u64,
        projections: &mut dyn super::tool_projection::ToolProjectionSink,
    ) -> Result<ToolExecutionOutcome, ToolCallError> {
        self.driver.execute_projected(
            &mut self.preparer,
            &mut self.coordinator,
            inputs,
            trust,
            decision_for,
            now,
            projections,
        )
    }
}
