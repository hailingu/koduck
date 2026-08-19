// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Authority-root reclamation legs: process-local Turn authority drops only
//! after the caller's durable probe proves the canonical Turn terminal and
//! forcibly retires every live or reserved execution mirror (ADR-0003 T-3,
//! TC-09/TC-12).

use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    AttemptStoreError, CanonicalTurnTerminal, ExecutionPreparationError, LeaseCheck,
    LeaseValidator, ToolAuthorizationService, ToolExecutionRuntime, ToolPolicyConfiguration,
};
use koduck_ai::domain::execution::{
    AttemptId, AuthorityReclamation, ExactActionBinding, ExecutionError, ExecutionStatus,
};
use koduck_ai::domain::tool::{Action, CapabilityDescriptor, DescriptorState, Effect};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TurnId};

use super::new_runtime;

/// Lease double answering Current so preparation reaches the authority.
#[derive(Clone, Copy)]
struct CurrentLease;

impl LeaseValidator for CurrentLease {
    fn check_current(&mut self, _binding: &ExactActionBinding) -> LeaseCheck {
        LeaseCheck::Current
    }
}

/// Canonical-terminal probe double with a scripted answer.
#[derive(Clone, Copy)]
struct ScriptedTerminal {
    terminal: bool,
    unavailable: bool,
}

impl CanonicalTurnTerminal for ScriptedTerminal {
    fn turn_is_terminal(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        if self.unavailable {
            Err(AttemptStoreError::Unavailable)
        } else {
            Ok(self.terminal)
        }
    }
}

fn proven_terminal() -> ScriptedTerminal {
    ScriptedTerminal {
        terminal: true,
        unavailable: false,
    }
}

fn unproven_terminal() -> ScriptedTerminal {
    ScriptedTerminal {
        terminal: false,
        unavailable: false,
    }
}

fn unavailable_terminal() -> ScriptedTerminal {
    ScriptedTerminal {
        terminal: false,
        unavailable: true,
    }
}

/// Read-only policy fixture: an in-profile `read_data` action needs no D-6,
/// so the guarded dispatch path claims and terminalizes without approvals.
struct ReadDataPolicy {
    descriptor: CapabilityDescriptor,
    profile: koduck_ai::domain::tool::PermissionProfile,
}

impl ReadDataPolicy {
    fn new() -> Self {
        Self {
            descriptor: CapabilityDescriptor::new(
                "fixture.read",
                "v1",
                Effect::ReadData,
                DescriptorState::Active,
                parse_input_schema(
                    r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
                )
                .expect("valid schema"),
            )
            .expect("valid descriptor"),
            profile: koduck_ai::domain::tool::PermissionProfile::builder("profile-default", "v1")
                .expect("valid profile")
                .allow("fixture.read", "v1", Effect::ReadData, "fixture-target")
                .expect("valid profile entry")
                .build(),
        }
    }
}

impl ToolPolicyConfiguration for ReadDataPolicy {
    fn descriptor_for(
        &self,
        action: &koduck_ai::domain::tool::Action,
    ) -> Option<&CapabilityDescriptor> {
        (self.descriptor.id() == action.descriptor_id()).then_some(&self.descriptor)
    }

    fn profile_for(
        &self,
        profile_id: &str,
        profile_version: &str,
    ) -> Option<&koduck_ai::domain::tool::PermissionProfile> {
        (self.profile.id() == profile_id && self.profile.version() == profile_version)
            .then_some(&self.profile)
    }
}

fn binding_for(tenant: &TenantId, thread: ThreadId, turn: TurnId) -> ExactActionBinding {
    let action = Action::new(
        "fixture.read",
        "v1",
        Effect::ReadData,
        "fixture-target",
        parse_action_parameters("{}").expect("valid parameters"),
    )
    .expect("valid action");
    let binding = ExactActionBinding::new(
        tenant.clone(),
        thread,
        turn,
        LeaseGeneration::initial(),
        ("profile-default", "v1"),
        AttemptId::new(),
        action,
    )
    .expect("valid binding");
    ToolAuthorizationService::new(ReadDataPolicy::new())
        .authorize_binding(binding)
        .expect("fixture binding is authorized")
}

/// Prepares one attempt on `runtime` for the supplied Turn identity.
fn prepare_on(
    runtime: &ToolExecutionRuntime,
    binding: &ExactActionBinding,
) -> Result<
    (
        koduck_ai::domain::execution::TurnExecutionAuthority,
        koduck_ai::domain::execution::ExecutionAttempt,
    ),
    ExecutionPreparationError,
> {
    let mut preparer = runtime.preparer(CurrentLease);
    preparer.prepare(binding.clone())
}

/// Moves one cataloged attempt to a durable terminal through the guarded
/// dispatch-claim and reservation path.
fn finish_prepared(
    authority: &mut koduck_ai::domain::execution::TurnExecutionAuthority,
    attempt: &mut koduck_ai::domain::execution::ExecutionAttempt,
) {
    authority
        .claim_dispatch(attempt, None, 1_000)
        .expect("the cataloged attempt claims its dispatch");
    authority
        .reserve_terminal(attempt)
        .expect("the cataloged attempt reserves its terminal");
    authority
        .mirror_terminal(attempt, ExecutionStatus::Succeeded)
        .expect("the reserved attempt mirrors its terminal");
}

#[test]
fn reclamation_drops_a_fully_terminal_authority_and_resets_only_local_slots() {
    let runtime = new_runtime();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();

    // Exhaust the Turn's sixteen slots; every attempt reaches a terminal
    // through the guarded path.
    for _ in 0..16 {
        let binding = binding_for(&tenant, thread, turn);
        let (mut authority, mut attempt) =
            prepare_on(&runtime, &binding).expect("a fresh slot prepares");
        finish_prepared(&mut authority, &mut attempt);
    }
    let seventeenth = binding_for(&tenant, thread, turn);
    assert!(matches!(
        prepare_on(&runtime, &seventeenth),
        Err(ExecutionPreparationError::Rejected(
            ExecutionError::AttemptLimit
        ))
    ));

    // The durable probe proves the canonical Turn terminal, so reclamation
    // drops the authority. A later allocation resets only process-local
    // slots: every durable D-7 write requires the Turn's `started` status,
    // so a terminal Turn can never resurrect its durable attempt budget.
    assert_eq!(
        runtime.reclaim_terminated(&mut proven_terminal(), &tenant, thread, turn),
        AuthorityReclamation::Reclaimed,
        "a fully terminal authority reclaims after the durable probe proves the terminal"
    );
    prepare_on(&runtime, &seventeenth)
        .expect("a reclaimed Turn allocates fresh process-local slots");
}

#[test]
fn reclamation_retires_a_live_local_attempt_after_a_proven_turn_terminal() {
    let runtime = new_runtime();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let binding = binding_for(&tenant, thread, turn);
    let (authority, _attempt) = prepare_on(&runtime, &binding).expect("a live attempt prepares");

    assert_eq!(
        runtime.reclaim_terminated(&mut proven_terminal(), &tenant, thread, turn),
        AuthorityReclamation::Reclaimed,
        "a proven Turn terminal retires a stale prepared D-7 mirror and releases authority"
    );
    assert!(
        authority.live_attempts().is_empty(),
        "the retired local mirror cannot remain interruptible after canonical closure"
    );
}

#[test]
fn reclamation_seals_a_cached_preparer_before_detaching_authority() {
    // This must reject before the local allocation: a preparer that cached its
    // authority before canonical closure cannot recreate a detached D-7
    // mirror after reclamation.
    let runtime = new_runtime();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let mut preparer = runtime.preparer(CurrentLease);

    preparer
        .prepare(binding_for(&tenant, thread, turn))
        .expect("the initial slot prepares");
    assert_eq!(
        runtime.reclaim_terminated(&mut proven_terminal(), &tenant, thread, turn),
        AuthorityReclamation::Reclaimed,
        "a proven terminal releases the cataloged authority"
    );

    assert!(matches!(
        preparer.prepare(binding_for(&tenant, thread, turn)),
        Err(ExecutionPreparationError::Rejected(
            ExecutionError::InterruptionRequested
        ))
    ));
}

#[test]
fn reclamation_retires_a_terminal_reservation_after_a_proven_turn_terminal() {
    let runtime = new_runtime();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();
    let binding = binding_for(&tenant, thread, turn);
    let (mut authority, attempt) = prepare_on(&runtime, &binding).expect("a live attempt prepares");
    authority
        .reserve_terminal(&attempt)
        .expect("the cataloged attempt reserves its terminal");

    assert_eq!(
        runtime.reclaim_terminated(&mut proven_terminal(), &tenant, thread, turn),
        AuthorityReclamation::Reclaimed,
        "a proven Turn terminal retires a stale terminal reservation and releases authority"
    );
    authority.release_terminal_reservation(&attempt);
    assert!(
        authority.reserve_terminal(&attempt).is_err(),
        "a stale caller cannot reopen the retired terminal reservation"
    );
}

#[test]
fn reclamation_retains_every_authority_without_a_proven_terminal() {
    let runtime = new_runtime();
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let thread = ThreadId::new();
    let turn = TurnId::new();

    for probe in [unproven_terminal(), unavailable_terminal()] {
        assert_eq!(
            runtime.reclaim_terminated(&mut { probe }, &tenant, thread, turn),
            AuthorityReclamation::Retained,
            "an unproven or unavailable probe retains the authority"
        );
    }
}

/// Probe double observing how many terminal notifications run concurrently.
///
/// Each notification sleeps briefly inside the probe so a shared lock around
/// the probe serializes the callers, while per-notification probes overlap.
#[derive(Clone, Default)]
struct SaturatingProbe {
    in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    max_in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl CanonicalTurnTerminal for SaturatingProbe {
    fn turn_is_terminal(
        &mut self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError> {
        use std::sync::atomic::Ordering;
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(25));
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(false)
    }
}

#[test]
fn concurrent_terminal_notifications_never_serialize_on_one_probe() {
    // Background recovery invokes the terminal observer while holding its
    // admission permit, and the bounded durable probe can take up to its
    // two-second deadline. Notifications must therefore probe concurrently —
    // one probe clone per notification — instead of queueing behind a shared
    // lock: 256 serialized probes would retain every admission permit for
    // roughly 512 seconds and starve later recovery scheduling
    // (ADR-0003 T-3, resource bounds and backpressure).
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::sync::atomic::Ordering;

    let root = koduck_ai::application::ToolExecutionRuntimeRoot::issue();
    let probe = SaturatingProbe::default();
    let observer =
        koduck_ai::runtime::tool_executor::AuthorityTerminalObserver::new(&root, probe.clone());
    let tenant = Arc::new(TenantId::new("tenant-a").expect("valid tenant"));
    let thread = Arc::new(ThreadId::new());
    let turn = Arc::new(TurnId::new());
    let contenders = 4;
    let barrier = Arc::new(Barrier::new(contenders));
    std::thread::scope(|scope| {
        for _ in 0..contenders {
            let observer = observer.clone();
            let barrier = Arc::clone(&barrier);
            let tenant = Arc::clone(&tenant);
            let thread = Arc::clone(&thread);
            let turn = Arc::clone(&turn);
            scope.spawn(move || {
                barrier.wait();
                koduck_ai::adapters::history::postgres::TurnTerminalObserver::terminal_may_have_committed(
                    &observer,
                    &tenant,
                    *thread,
                    *turn,
                );
            });
        }
    });

    assert!(
        probe.max_in_flight.load(Ordering::SeqCst) > 1,
        "terminal notifications must probe concurrently, found a serialized maximum of {}",
        probe.max_in_flight.load(Ordering::SeqCst)
    );
}
