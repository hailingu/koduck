// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Production-path legs of the canonical `PostgreSQL` harness: the C-5 driver
//! and coordinator record every prepared D-7 and claim the single durable
//! running slot before any executor dispatch, so competing instances converge
//! on exactly one dispatch and one canonical terminal (ADR-0003 AC-12,
//! TC-09/TC-12).

use std::sync::Arc;
use std::sync::Barrier;
use std::sync::Mutex;

use koduck_ai::adapters::execution::SqlxTurnLeaseValidator;
use koduck_ai::adapters::tool::{parse_action_parameters, parse_input_schema};
use koduck_ai::application::{
    ActionDeadline, CancelAcknowledgement, CancelPermit, CanonicalTurnTerminal,
    DispatchClaimResolution, DispatchPermit, EffectState, ExecutionCoordinator, ExecutionFailure,
    ExecutionPending, ExecutionResponse, ExecutionResponseBuilder, ExecutorError, IsolatedExecutor,
    ToolAuthorizationService, ToolCallInputs, ToolConfigurationSnapshot, ToolExecutionAssembly,
    ToolExecutionOutcome, ToolExecutionRuntimeRoot,
};
use koduck_ai::domain::execution::{ApprovalDecision, AttemptId, ExactActionBinding};
use koduck_ai::domain::tool::{
    Action, CapabilityDescriptor, DescriptorState, Effect, PermissionProfile,
};
use koduck_ai::domain::{LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};
use uuid::Uuid;

use super::FixturePolicyConfiguration;
use super::attempts::{attempt_store, seed_owner_rows};
use super::harness;

const STARTED_AT_MILLIS: u64 = 5_000;

/// Executor that records every dispatched D-7 identity and returns one bounded
/// successful response. Every competing C-5 instance shares one executor, so
/// the dispatch log is the observable executor boundary the AC-12 race counts.
#[derive(Clone, Default)]
struct SharedCountingExecutor {
    dispatches: Arc<Mutex<Vec<AttemptId>>>,
}

impl SharedCountingExecutor {
    fn dispatch_count(&self) -> usize {
        self.dispatches
            .lock()
            .expect("dispatch log is healthy")
            .len()
    }
}

impl IsolatedExecutor for SharedCountingExecutor {
    fn execute(
        &mut self,
        _permit: &DispatchPermit,
        binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> Result<ExecutionResponse, ExecutorError> {
        self.dispatches
            .lock()
            .expect("dispatch log is healthy")
            .push(binding.attempt_id());
        let mut response = ExecutionResponseBuilder::new(EffectState::Started);
        response
            .push_chunk(b"committed")
            .expect("fixture output is within the limit");
        response.finish()
    }

    fn cancel(
        &mut self,
        _permit: &CancelPermit,
        _binding: &ExactActionBinding,
        _deadline: ActionDeadline,
    ) -> CancelAcknowledgement {
        CancelAcknowledgement::NotAcknowledged
    }
}

fn fixture_action() -> Action {
    Action::new(
        "fixture.tool",
        "v1",
        Effect::ReadData,
        "fixture-target",
        parse_action_parameters(r"{}").expect("valid parameters"),
    )
    .expect("valid fixture action")
}

fn fixture_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        "fixture.tool",
        "v1",
        Effect::ReadData,
        DescriptorState::Active,
        parse_input_schema(
            r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
        )
        .expect("valid fixture schema"),
    )
    .expect("valid fixture descriptor")
}

fn fixture_profile() -> PermissionProfile {
    PermissionProfile::builder("profile-default", "v1")
        .expect("valid fixture profile")
        .allow("fixture.tool", "v1", Effect::ReadData, "fixture-target")
        .expect("valid fixture profile entry")
        .build()
}

fn fixture_snapshot() -> ToolConfigurationSnapshot {
    let mut snapshot = ToolConfigurationSnapshot::empty();
    snapshot
        .register_descriptor(fixture_descriptor())
        .expect("descriptor registration is unique");
    snapshot
        .register_profile(fixture_profile())
        .expect("profile registration is unique");
    snapshot
}

/// One read-data C-5 call identity: tenant, Thread, Turn, and generation.
struct CallIdentity {
    tenant: TenantId,
    thread: ThreadId,
    turn: TurnId,
    lease_generation: LeaseGeneration,
}

fn call_identity() -> CallIdentity {
    CallIdentity {
        tenant: TenantId::new(format!("ci-{}", Uuid::new_v4())).expect("valid tenant"),
        thread: ThreadId::new(),
        turn: TurnId::new(),
        lease_generation: LeaseGeneration::initial(),
    }
}

impl CallIdentity {
    fn seed(&self, harness: &super::Harness) {
        seed_owner_rows(
            harness,
            &self.tenant,
            self.thread,
            self.turn,
            self.lease_generation,
        );
    }

    fn trust(&self) -> TrustContext {
        TrustContext::new(self.tenant.clone(), "subject-a").expect("valid principal")
    }

    fn inputs(&self) -> ToolCallInputs {
        ToolCallInputs {
            tenant_id: self.tenant.clone(),
            thread_id: self.thread,
            turn_id: self.turn,
            lease_generation: self.lease_generation,
            profile_id: String::from("profile-default"),
            profile_version: String::from("v1"),
            action: fixture_action(),
            turn_deadline_millis: u64::MAX,
        }
    }

    /// Returns every canonical D-7 row of this Turn as (`status`, `version`,
    /// `started_at_millis`) tuples in attempt order.
    fn canonical_rows(&self, harness: &super::Harness) -> Vec<(String, i64, Option<i64>)> {
        harness
            .runtime
            .block_on(async {
                sqlx::query_as(
                    "SELECT status, version, started_at_millis \
                     FROM tool_execution_attempts \
                     WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3 \
                     ORDER BY prepared_at_millis, attempt_id",
                )
                .bind(self.tenant.as_str())
                .bind(self.thread.as_uuid())
                .bind(self.turn.as_uuid())
                .fetch_all(&harness.pool)
                .await
            })
            .expect("canonical D-7 rows are readable")
    }
}

/// Builds one policy-sealed read-data binding whose durable row the C-5
/// production path owns, with its canonical C-6 owner rows already seeded.
fn sealed_binding(harness: &super::Harness) -> (CallIdentity, ExactActionBinding) {
    let identity = call_identity();
    identity.seed(harness);
    let binding = ExactActionBinding::new(
        identity.tenant.clone(),
        identity.thread,
        identity.turn,
        identity.lease_generation,
        ("profile-default", "v1"),
        AttemptId::new(),
        fixture_action(),
    )
    .expect("valid binding");
    let sealed = ToolAuthorizationService::new(FixturePolicyConfiguration {
        descriptor: fixture_descriptor(),
        profile: fixture_profile(),
    })
    .authorize_binding(binding)
    .expect("fixture binding is policy-authorized");
    (identity, sealed)
}

/// Seeds and returns one policy-sealed D-7 binding for sibling harness legs.
pub(super) fn prepared_sealed_binding(harness: &super::Harness) -> ExactActionBinding {
    sealed_binding(harness).1
}

#[test]
fn driver_commits_one_durable_attempt_end_to_end() {
    let Some(harness) = harness() else {
        return;
    };
    let identity = call_identity();
    identity.seed(&harness);
    let store = attempt_store(harness.pool.clone(), &harness.runtime);
    let lease = SqlxTurnLeaseValidator::new(harness.pool.clone(), harness.runtime.handle().clone());
    let executor = SharedCountingExecutor::default();
    let root = ToolExecutionRuntimeRoot::issue();
    let assembly = ToolExecutionAssembly::new(&root, fixture_snapshot());
    let mut boundary = assembly.boundary(executor.clone(), lease, store);

    let mut decision = |_request: &koduck_ai::domain::execution::ApprovalRequest| {
        (ApprovalDecision::Cancelled, 0_u64)
    };
    let mut now = || STARTED_AT_MILLIS;
    let outcome = boundary
        .execute(
            &identity.inputs(),
            &identity.trust(),
            &mut decision,
            &mut now,
        )
        .expect("the production driver path commits one durable attempt end to end");

    assert!(
        matches!(outcome, ToolExecutionOutcome::Succeeded { .. }),
        "the wired durable path returns the committed terminal, found {outcome:?}"
    );
    assert_eq!(
        executor.dispatch_count(),
        1,
        "the single canonical D-7 dispatches exactly once"
    );
    // The durable row records the full canonical progression: prepared at
    // version 1, claimed running at version 2 with the dispatch start time,
    // and the committed terminal at version 3.
    assert_eq!(
        identity.canonical_rows(&harness),
        vec![(
            "succeeded".to_owned(),
            3,
            Some(i64::try_from(STARTED_AT_MILLIS).expect("start fits"))
        )],
        "exactly one durable D-7 row reaches the committed terminal"
    );
}

#[test]
fn production_dispatch_claims_permit_exactly_one_executor_dispatch_across_instances() {
    let Some(harness) = harness() else {
        return;
    };
    let contenders = 32;
    let (identity, sealed) = sealed_binding(&harness);
    // One canonical prepared D-7 exists before the race: every instance
    // prepares the same identity into its own process authority and then
    // contends for the single durable running slot.
    let mut seeder = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(&mut seeder, &sealed, 1_000),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );

    let executor = SharedCountingExecutor::default();
    let barrier = Arc::new(Barrier::new(contenders));
    let results: Vec<Result<ToolExecutionOutcome, ExecutionPending>> =
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..contenders {
                let executor = executor.clone();
                let store = attempt_store(harness.pool.clone(), &harness.runtime);
                let lease = SqlxTurnLeaseValidator::new(
                    harness.pool.clone(),
                    harness.runtime.handle().clone(),
                );
                let sealed = sealed.clone();
                let barrier = Arc::clone(&barrier);
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    // Each contender is one process: a fresh authority root and
                    // its own catalog, sharing only the durable store, the lease
                    // validator, and the executor boundary.
                    let root = ToolExecutionRuntimeRoot::issue();
                    let mut preparer = root.runtime().preparer(lease.clone());
                    let (mut authority, mut attempt) = preparer
                        .prepare(sealed.clone())
                        .expect("each instance prepares the canonical D-7 locally");
                    let mut coordinator = ExecutionCoordinator::new(executor, lease, store);
                    let mut now = || STARTED_AT_MILLIS;
                    coordinator.execute(
                        &mut authority,
                        None,
                        &mut attempt,
                        STARTED_AT_MILLIS,
                        &mut now,
                    )
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().expect("dispatch contender completes"))
                .collect()
        });

    let succeeded = results
        .iter()
        .filter(|result| matches!(result, Ok(ToolExecutionOutcome::Succeeded { .. })))
        .count();
    let reconciliation = results
        .iter()
        .filter(|result| {
            matches!(
                result,
                Err(ExecutionPending::ReconciliationRequired {
                    code: ExecutionFailure::TerminalConflict,
                    ..
                })
            )
        })
        .count();
    assert_eq!(
        executor.dispatch_count(),
        1,
        "exactly one instance dispatches the single canonical D-7 across 32 contenders"
    );
    assert_eq!(
        succeeded, 1,
        "only the durable claim winner commits a terminal"
    );
    assert_eq!(
        reconciliation,
        contenders - 1,
        "every other instance fails closed without dispatch, found {results:?}"
    );
    assert_eq!(
        identity.canonical_rows(&harness),
        vec![(
            "succeeded".to_owned(),
            3,
            Some(i64::try_from(STARTED_AT_MILLIS).expect("start fits"))
        )],
        "the canonical D-7 keeps exactly one terminal"
    );
}

#[test]
fn canonical_terminal_probe_gates_durable_authority_reclamation() {
    // A Turn with a live process-local authority reclaims only after the
    // durable probe observes its canonical terminal: a `started` Turn
    // retains the authority, the durable terminal releases it, and a
    // missing Turn row never authorizes reclamation (ADR-0003 T-3).
    let Some(harness) = harness() else {
        return;
    };
    let (identity, sealed) = sealed_binding(&harness);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        store.turn_is_terminal(&identity.tenant, identity.thread, identity.turn),
        Ok(false),
        "a started canonical Turn is not terminal"
    );
    let missing = call_identity();
    assert_eq!(
        store.turn_is_terminal(&missing.tenant, missing.thread, missing.turn),
        Ok(false),
        "a missing canonical Turn row proves nothing"
    );

    let root = ToolExecutionRuntimeRoot::issue();
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let (mut authority, mut attempt) = preparer
        .prepare(sealed.clone())
        .expect("the live fixture attempt prepares locally");
    authority
        .claim_dispatch(&mut attempt, None, STARTED_AT_MILLIS)
        .expect("the fixture attempt claims its dispatch");
    authority
        .reserve_terminal(&attempt)
        .expect("the fixture attempt reserves its terminal");
    authority
        .mirror_terminal(
            &mut attempt,
            koduck_ai::domain::execution::ExecutionStatus::Failed,
        )
        .expect("the fixture attempt mirrors its terminal");

    assert_eq!(
        root.runtime().reclaim_terminated(
            &mut store,
            &identity.tenant,
            identity.thread,
            identity.turn
        ),
        koduck_ai::domain::execution::AuthorityReclamation::Retained,
        "a started canonical Turn retains even a fully terminal local authority"
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET status = 'completed' \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(identity.tenant.as_str())
        .bind(identity.thread.as_uuid())
        .bind(identity.turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn reaches its canonical terminal");
    });
    assert_eq!(
        store.turn_is_terminal(&identity.tenant, identity.thread, identity.turn),
        Ok(true),
        "the durable terminal is proven"
    );
    assert_eq!(
        root.runtime().reclaim_terminated(
            &mut store,
            &identity.tenant,
            identity.thread,
            identity.turn
        ),
        koduck_ai::domain::execution::AuthorityReclamation::Reclaimed,
        "the proven canonical terminal releases the process-local authority"
    );
}

#[test]
fn canonical_terminal_probe_retains_authority_during_recovery_pending() {
    // `recovery-pending` is an intermediate durable lifecycle state. It must
    // not release C-5's process-local authority because recovery still owns
    // the Turn and may commit its final terminal (ADR-0003 T-3).
    let Some(harness) = harness() else {
        return;
    };
    let identity = call_identity();
    identity.seed(&harness);
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET status = 'recovery-pending' \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(identity.tenant.as_str())
        .bind(identity.thread.as_uuid())
        .bind(identity.turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture turn enters recovery-pending");
    });

    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        store.turn_is_terminal(&identity.tenant, identity.thread, identity.turn),
        Ok(false),
        "recovery-pending remains non-terminal until durable recovery commits its terminal"
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one durable leg covering the fenced, replayed, and renewed-lease outcomes of the dedicated transition"
)]
fn fenced_post_dispatch_started_effect_persists_owner_fenced_failure() {
    // ADR-0003 lines 309-314: fencing after dispatch with a started or
    // unknown effect persists failed/owner_fenced_after_dispatch through the
    // dedicated reconciliation-capable transition — not the lease-expiry
    // fallback's timed_out/unknown — while a still-current lease can never be
    // relabelled through it (TC-07/TC-12).
    let Some(harness) = harness() else {
        return;
    };
    let (identity, sealed) = sealed_binding(&harness);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(&mut store, &sealed, 1_000),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(&mut store, &sealed, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );

    // A still-current lease must not be relabelled through the transition.
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::commit_fenced_after_dispatch(
            &mut store,
            &sealed,
            EffectState::Started,
            3_000,
        ),
        Ok(koduck_ai::application::AttemptTerminalResolution::Conflict),
        "a current lease keeps the current-generation terminal path"
    );

    // Fence the bound lease; the canonical failure commits exactly once and
    // replays idempotently.
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(identity.tenant.as_str())
        .bind(identity.thread.as_uuid())
        .bind(identity.turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture lease is fenced");
    });
    let expected_terminal = koduck_ai::application::AttemptTerminalResolution::Won { version: 3 };
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::commit_fenced_after_dispatch(
            &mut store,
            &sealed,
            EffectState::Started,
            3_000,
        ),
        Ok(expected_terminal),
        "the fenced post-dispatch failure persists as the canonical terminal"
    );
    let replay = koduck_ai::application::ExecutionAttemptStore::commit_fenced_after_dispatch(
        &mut store,
        &sealed,
        EffectState::Started,
        4_000,
    );
    assert!(
        matches!(
            replay,
            Ok(koduck_ai::application::AttemptTerminalResolution::ExistingTerminal(_))
        ),
        "the second fenced commit replays the committed terminal, found {replay:?}"
    );

    // A lease that expired and then renewed back to current must never be
    // relabelled through the fenced transition: the write locks and
    // evaluates the exact lease row in its own transaction, so a concurrent
    // heartbeat serializes with — and can prevent — the failure write
    // (ADR-0003 TC-07/TC-12).
    let (renewed_identity, sealed_renewed) = sealed_binding(&harness);
    renewed_identity.seed(&harness);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(
            &mut store,
            &sealed_renewed,
            1_500
        ),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(
            &mut store,
            &sealed_renewed,
            2_500
        ),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
    let renewed_outcome =
        koduck_ai::application::ExecutionAttemptStore::commit_fenced_after_dispatch(
            &mut store,
            &sealed_renewed,
            EffectState::Started,
            3_500,
        );
    assert_ne!(
        renewed_outcome,
        Ok(koduck_ai::application::AttemptTerminalResolution::Won { version: 3 }),
        "a current lease is never relabelled through the fenced transition, found {renewed_outcome:?}"
    );
    assert_eq!(
        identity.canonical_rows(&harness),
        vec![("failed".to_owned(), 3, Some(2_000))],
        "the canonical row carries failed at version 3"
    );
    let failure_code: Option<String> = harness
        .runtime
        .block_on(async {
            sqlx::query_scalar(
                "SELECT failure_code FROM tool_execution_attempts \
                 WHERE tenant_id = $1 AND attempt_id = $2",
            )
            .bind(identity.tenant.as_str())
            .bind(sealed.attempt_id().as_uuid())
            .fetch_one(&harness.pool)
            .await
        })
        .expect("failure code is readable");
    assert_eq!(failure_code.as_deref(), Some("owner_fenced_after_dispatch"));
}

#[test]
fn normal_terminal_commits_stop_after_the_interruption_barrier() {
    // An authenticated interruption committed on another replica sets the
    // durable Turn barrier while the owning replica's lease is still live for
    // its remaining window; a normal current-generation terminal (for example
    // `succeeded`) must not commit through that barrier — the write loses to
    // the typed conflict and expiry reconciliation closes the attempt
    // (ADR-0003 TC-10/TC-12).
    let Some(harness) = harness() else {
        return;
    };
    let (identity, sealed) = sealed_binding(&harness);
    let mut store = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(&mut store, &sealed, 1_000),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(&mut store, &sealed, 2_000),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
    let succeeded = koduck_ai::application::DurableAttemptTerminal::from_outcome(
        &koduck_ai::application::ToolExecutionOutcome::Succeeded {
            output: b"ok".to_vec(),
            effect_state: EffectState::Started,
        },
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::commit_terminal(
            &mut store, &sealed, &succeeded, 3_000
        ),
        Ok(koduck_ai::application::AttemptTerminalResolution::Won { version: 3 }),
        "an unbarriered running attempt commits its terminal normally"
    );

    // A second attempt under a freshly committed interruption barrier loses
    // the normal terminal write even though its lease row is still current.
    let loser = koduck_ai::domain::execution::ExactActionBinding::new(
        identity.tenant.clone(),
        identity.thread,
        identity.turn,
        identity.lease_generation,
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        fixture_action(),
    )
    .expect("valid loser binding");
    let sealed_loser = ToolAuthorizationService::new(super::FixturePolicyConfiguration {
        descriptor: fixture_descriptor(),
        profile: fixture_profile(),
    })
    .authorize_binding(loser)
    .expect("loser binding is policy-authorized");
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(
            &mut store,
            &sealed_loser,
            1_500
        ),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(
            &mut store,
            &sealed_loser,
            2_500
        ),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turns SET interrupting = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(identity.tenant.as_str())
        .bind(identity.thread.as_uuid())
        .bind(identity.turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture barrier committed");
    });
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::commit_terminal(
            &mut store,
            &sealed_loser,
            &succeeded,
            3_500
        ),
        Ok(koduck_ai::application::AttemptTerminalResolution::Conflict),
        "the normal terminal write loses to the committed interruption barrier"
    );
}

/// Lease validator double answering Current for every check: these legs
/// isolate the durable store's own fencing with a controlled foreground
/// answer, while the production runtime wires the durable C-6 check into
/// both the dispatch and interruption paths.
#[derive(Clone, Copy)]
struct AlwaysCurrentLease;

impl koduck_ai::application::LeaseValidator for AlwaysCurrentLease {
    fn check_current(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> koduck_ai::application::LeaseCheck {
        koduck_ai::application::LeaseCheck::Current
    }
}

#[test]
fn fenced_durable_claim_defers_to_reconciliation_without_mutation() {
    // The durable lease is fenced, so the claim reports Fenced and the
    // coordinator must defer to reconciliation: every terminal write
    // requires the current lease, so no close is attempted and the canonical
    // row stays prepared at version 1 — the accepted stale-owner semantics
    // of the interruption legs (ADR-0003 TC-07/AC-8).
    let Some(harness) = harness() else {
        return;
    };
    let (identity, sealed) = sealed_binding(&harness);
    let mut seeder = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(&mut seeder, &sealed, 1_000),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    harness.runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(identity.tenant.as_str())
        .bind(identity.thread.as_uuid())
        .bind(identity.turn.as_uuid())
        .execute(&harness.pool)
        .await
        .expect("fixture lease is fenced");
    });

    let executor = SharedCountingExecutor::default();
    let root = ToolExecutionRuntimeRoot::issue();
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let (mut authority, mut attempt) = preparer
        .prepare(sealed.clone())
        .expect("the process-local preparation succeeds");
    let mut coordinator = ExecutionCoordinator::new(
        executor.clone(),
        AlwaysCurrentLease,
        attempt_store(harness.pool.clone(), &harness.runtime),
    );
    let mut now = || STARTED_AT_MILLIS;
    let result = coordinator.execute(
        &mut authority,
        None,
        &mut attempt,
        STARTED_AT_MILLIS,
        &mut now,
    );

    assert!(
        matches!(
            result,
            Err(ExecutionPending::ReconciliationRequired {
                code: ExecutionFailure::OwnerFencedBeforeDispatch,
                ..
            })
        ),
        "a fenced durable claim defers to reconciliation, found {result:?}"
    );
    assert_eq!(executor.dispatch_count(), 0);
    assert_eq!(
        identity.canonical_rows(&harness),
        vec![("prepared".to_owned(), 1, None)],
        "the canonical row keeps its still-prepared state without mutation"
    );
}

#[test]
fn concurrent_claim_closes_only_its_own_still_prepared_attempt() {
    // Another D-7 owns the Turn's running slot, so this prepared attempt's
    // claim reports Concurrent: the coordinator closes its own still-prepared
    // row through the prepared-only compare-and-set and reports the typed
    // rejection, while the slot owner's running row is untouched
    // (ADR-0003 TC-09/TC-12).
    let Some(harness) = harness() else {
        return;
    };
    let (identity, owner_binding) = sealed_binding(&harness);
    let mut seeder = attempt_store(harness.pool.clone(), &harness.runtime);
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(
            &mut seeder,
            &owner_binding,
            1_000
        ),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );
    // The slot owner claims first; its attempt stays running.
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::claim_running(
            &mut seeder,
            &owner_binding,
            2_000
        ),
        Ok(DispatchClaimResolution::Claimed { version: 2 }),
    );

    // A second identity of the same Turn prepares and loses the claim race.
    let loser_binding = koduck_ai::domain::execution::ExactActionBinding::new(
        identity.tenant.clone(),
        identity.thread,
        identity.turn,
        identity.lease_generation,
        ("profile-default", "v1"),
        koduck_ai::domain::execution::AttemptId::new(),
        fixture_action(),
    )
    .expect("valid loser binding");
    let sealed_loser = ToolAuthorizationService::new(super::FixturePolicyConfiguration {
        descriptor: fixture_descriptor(),
        profile: fixture_profile(),
    })
    .authorize_binding(loser_binding)
    .expect("loser binding is policy-authorized");
    assert_eq!(
        koduck_ai::application::ExecutionAttemptStore::insert_prepared(
            &mut seeder,
            &sealed_loser,
            1_000
        ),
        Ok(koduck_ai::application::AttemptInsertResolution::Inserted),
    );

    let executor = SharedCountingExecutor::default();
    let root = ToolExecutionRuntimeRoot::issue();
    let mut preparer = root.runtime().preparer(AlwaysCurrentLease);
    let (mut authority, mut attempt) = preparer
        .prepare(sealed_loser.clone())
        .expect("the loser prepares locally");
    let mut coordinator = ExecutionCoordinator::new(
        executor.clone(),
        AlwaysCurrentLease,
        attempt_store(harness.pool.clone(), &harness.runtime),
    );
    let mut now = || STARTED_AT_MILLIS;
    let result = coordinator.execute(
        &mut authority,
        None,
        &mut attempt,
        STARTED_AT_MILLIS,
        &mut now,
    );

    assert_eq!(
        result,
        Err(ExecutionPending::DispatchRejected {
            code: ExecutionFailure::ConcurrentAttempt,
        }),
        "the concurrent loser receives the typed rejection"
    );
    assert_eq!(executor.dispatch_count(), 0);
    let mut rows = identity.canonical_rows(&harness);
    rows.sort_by_key(|(status, _, _)| status.clone());
    assert_eq!(
        rows,
        vec![
            ("cancelled".to_owned(), 3, None),
            ("running".to_owned(), 2, Some(2_000)),
        ],
        "the loser's still-prepared row closes cancelled while the slot owner's running row is untouched"
    );
}
