// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! AC-8 durable integration leg: an authenticated interruption commits the
//! D-7 terminal through the real durable conditional store and leaves the
//! same Turn with exactly one durable terminal in the production `PostgreSQL`
//! history, whose canonical replay returns it.

use koduck_ai::adapters::execution::{SqlxExecutionAttemptStore, SqlxTurnLeaseValidator};
use koduck_ai::adapters::history::postgres::{PostgresTurnHistory, SqlxPostgresExecutor};
use std::sync::{Arc, Mutex, mpsc};

use koduck_ai::application::{
    AcceptedTurn, AttemptCommitError, AttemptCommitResult, AttemptCommitter,
    AttemptInsertResolution, AttemptStoreError, AttemptTerminalResolution,
    CanonicalAttemptTerminal, DispatchClaimResolution, DurableAttemptTerminal,
    DurableAttemptTransitions, ExecutionAttemptInterruptionGuard, ExecutionAttemptLiveness,
    ExecutionAttemptStore, HistoryError, ModelInput, ModelProvider, PreparedCloseResolution,
    ProviderError, ProviderStream, ToolExecutionOutcome, TurnCommand, TurnHistory, TurnRunner,
};
use koduck_ai::domain::execution::ExactActionBinding;
use koduck_ai::domain::{ItemPayload, TenantId, TerminalOutcome, Usage};
use koduck_ai::runtime::RuntimeState;

use super::*;

/// Provider guard for an interruption-only runner path; no stream is started.
struct NoopProvider;

impl ModelProvider for NoopProvider {
    fn stream(&mut self, _input: ModelInput) -> Result<ProviderStream<'_>, ProviderError> {
        Err(ProviderError {
            code: "unexpected-provider-call".to_owned(),
        })
    }
}

/// Connects the durable D-7 store and the production canonical history over a
/// disposable `PostgreSQL`, or `None` when no test database is configured. The
/// raw pool accompanies them so stale-ownership legs can fence or expire the
/// canonical lease row directly.
pub(super) fn durable_backends() -> Option<(
    SqlxExecutionAttemptStore,
    PostgresTurnHistory<SqlxPostgresExecutor>,
    sqlx::PgPool,
    tokio::runtime::Runtime,
)> {
    let database_url = std::env::var("KODUCK_AI_TEST_DATABASE_URL").ok()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("durable leg runtime");
    let pool = runtime
        .block_on(async { sqlx::PgPool::connect(&database_url).await })
        .expect("connect to disposable PostgreSQL");
    // The shared process-wide guard serializes this DDL against the parallel
    // env-gated harnesses in the same test binary.
    runtime.block_on(crate::test_migrations::ensure(&pool));
    Some((
        SqlxExecutionAttemptStore::new(pool.clone(), runtime.handle().clone()),
        PostgresTurnHistory::new(SqlxPostgresExecutor::new(
            pool.clone(),
            runtime.handle().clone(),
        )),
        pool,
        runtime,
    ))
}

/// Durable store wrapper that proves a remote D-7 can be introduced after the
/// runner observed no process-local work but before it records the Turn
/// terminal. The production fix must establish its durable interruption
/// barrier before this lookup is released.
#[derive(Clone)]
struct PausingLivenessStore {
    inner: SqlxExecutionAttemptStore,
    checked: mpsc::Sender<()>,
    release: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl AttemptCommitter for PausingLivenessStore {
    fn commit_outcome(
        &mut self,
        binding: &ExactActionBinding,
        outcome: &ToolExecutionOutcome,
    ) -> Result<AttemptCommitResult, AttemptCommitError> {
        self.inner.commit_outcome(binding, outcome)
    }
}

impl koduck_ai::application::CanonicalTurnTerminal for PausingLivenessStore {
    fn turn_is_terminal(
        &mut self,
        tenant_id: &TenantId,
        thread_id: koduck_ai::domain::ThreadId,
        turn_id: koduck_ai::domain::TurnId,
    ) -> Result<bool, AttemptStoreError> {
        self.inner.turn_is_terminal(tenant_id, thread_id, turn_id)
    }
}

impl DurableAttemptTransitions for PausingLivenessStore {
    fn insert_prepared(
        &mut self,
        binding: &ExactActionBinding,
        prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        // The durable leg drives the real canonical transitions, so the
        // wrapper delegates instead of fabricating process-local answers.
        ExecutionAttemptStore::insert_prepared(&mut self.inner, binding, prepared_at_millis)
    }

    fn claim_running(
        &mut self,
        binding: &ExactActionBinding,
        started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        ExecutionAttemptStore::claim_running(&mut self.inner, binding, started_at_millis)
    }

    fn cancel_prepared_attempt(
        &mut self,
        binding: &ExactActionBinding,
    ) -> Result<PreparedCloseResolution, AttemptStoreError> {
        ExecutionAttemptStore::cancel_prepared_attempt(&mut self.inner, binding)
    }
}

impl ExecutionAttemptLiveness for PausingLivenessStore {
    fn has_live_attempt(
        &mut self,
        tenant_id: &TenantId,
        thread_id: koduck_ai::domain::ThreadId,
        turn_id: koduck_ai::domain::TurnId,
    ) -> Result<bool, AttemptStoreError> {
        let live = self.inner.has_live_attempt(tenant_id, thread_id, turn_id)?;
        self.checked
            .send(())
            .expect("test observes the durable liveness result");
        self.release
            .lock()
            .expect("liveness release lock")
            .recv()
            .expect("test releases the liveness result");
        Ok(live)
    }

    fn unrecorded_terminal_projections(
        &mut self,
        tenant_id: &TenantId,
        thread_id: koduck_ai::domain::ThreadId,
        turn_id: koduck_ai::domain::TurnId,
    ) -> Result<Vec<koduck_ai::application::ToolProjection>, AttemptStoreError> {
        self.inner
            .unrecorded_terminal_projections(tenant_id, thread_id, turn_id)
    }
}

impl ExecutionAttemptInterruptionGuard for PausingLivenessStore {
    fn begin_interruption(
        &mut self,
        tenant_id: &TenantId,
        thread_id: koduck_ai::domain::ThreadId,
        turn_id: koduck_ai::domain::TurnId,
    ) -> Result<koduck_ai::application::InterruptionBarrierResolution, AttemptStoreError> {
        self.inner.begin_interruption(tenant_id, thread_id, turn_id)
    }
}

#[test]
fn interruption_leaves_one_durable_turn_terminal_and_replay() {
    let Some((mut durable, mut history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");

    // The production history mints the canonical Thread/Turn identity; the
    // interrupted D-7 below is bound to this exact Turn, so the D-7 terminal
    // and the arbitrated Turn terminal provably share one tenant/Thread/Turn.
    let command = TurnCommand::new(trust.clone(), None, "interrupt me").expect("valid command");
    let accepted = history
        .accept_initial(&command)
        .expect("initial acceptance");
    let runtime_state = RuntimeState::assemble();
    let harness = Harness::with_runtime(
        runtime_state.tool_execution_root().runtime(),
        tenant.clone(),
        accepted.thread_id,
        accepted.turn_id,
    );
    let (_authority, attempt) = harness.prepared();
    let binding: ExactActionBinding = attempt.binding().clone();

    // The prepared D-7 exists canonically at version 1 and is registered in
    // the same C-5 authority root used by the production runner executor.
    let mut seeded = durable.clone();
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut seeded, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    // The production runner composes the authenticated history lookup, C-5
    // prepared-attempt cancellation gated by the durable lease validator,
    // the durable D-7 terminal write, and the canonical CAND-1 Turn
    // interruption terminal in that order.
    let mut runner = TurnRunner::new(NoopProvider, history.clone()).with_tool_executor(
        runtime_state.tool_call_executor(
            durable.clone(),
            SqlxTurnLeaseValidator::new(pool, runtime.handle().clone()),
            koduck_ai::application::NoToolAudits,
            durable.clone(),
        ),
    );
    runner
        .request_interrupt(&trust, accepted.turn_id)
        .expect("production interruption cancels its prepared D-7 and terminalizes the Turn");

    // The durable D-7 row now carries exactly one canonical terminal: the
    // idempotent replay observes it and no state changes. `commit_terminal`
    // fixes the terminal record version at 3 (prepared 1, running 2,
    // terminal 3), and the store rejects a replayed non-3 terminal.
    let cancelled = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
        effect_state: EffectState::NotStarted,
    });
    assert_eq!(
        durable.commit_terminal(&binding, &cancelled, 4_000),
        Ok(AttemptTerminalResolution::ExistingTerminal(Box::new(
            CanonicalAttemptTerminal::from_persistence(
                binding.clone(),
                3,
                ToolExecutionOutcome::Cancelled {
                    effect_state: EffectState::NotStarted,
                },
            )
            .expect("canonical terminal validates"),
        ))),
    );

    // The interrupted D-7's own Turn keeps exactly one durable terminal.
    assert_single_arbitrated_turn_terminal(&mut history, &trust, &accepted);
}

#[test]
fn interruption_barrier_prevents_a_remote_dispatch_after_no_live_lookup() {
    let Some((mut durable, history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .clone()
        .accept_initial(
            &TurnCommand::new(trust.clone(), None, "interrupt race").expect("valid command"),
        )
        .expect("initial acceptance");
    let binding = sealed_binding(tenant.clone(), accepted.thread_id, accepted.turn_id);
    let (checked, checked_rx) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let pausing_store = PausingLivenessStore {
        inner: durable.clone(),
        checked,
        release: Arc::new(Mutex::new(release)),
    };
    let runtime_state = RuntimeState::assemble();
    let mut runner = TurnRunner::new(NoopProvider, history.clone()).with_tool_executor(
        runtime_state.tool_call_executor(
            pausing_store.clone(),
            SqlxTurnLeaseValidator::new(pool, runtime.handle().clone()),
            koduck_ai::application::NoToolAudits,
            pausing_store,
        ),
    );

    let interrupt = std::thread::spawn(move || runner.request_interrupt(&trust, accepted.turn_id));
    checked_rx
        .recv()
        .expect("runner observed the initial no-live result");

    // This prepared -> running transition is started strictly after the
    // runner's liveness read. It must be rejected by the durable interruption
    // barrier instead of becoming external work alongside an Interrupted Turn.
    let remote_insert = ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000);
    release_tx.send(()).expect("release runner");
    let interrupted = interrupt.join().expect("runner thread completes");

    assert_eq!(
        remote_insert,
        Err(AttemptStoreError::Unavailable),
        "a remote D-7 must not prepare or claim after interruption begins"
    );
    assert!(
        interrupted.is_ok(),
        "interruption completes after blocking dispatch"
    );
}

/// A lease state in which the requesting owner no longer owns the Turn.
#[derive(Clone, Copy)]
enum StaleLease {
    /// A newer generation fenced the bound generation.
    Fenced,
    /// The bound generation's lease expired without renewal.
    Expired,
}

#[test]
fn interruption_with_fenced_lease_mutates_no_canonical_state() {
    stale_owner_interruption_mutates_nothing(StaleLease::Fenced);
}

#[test]
fn interruption_with_expired_lease_mutates_no_canonical_state() {
    stale_owner_interruption_mutates_nothing(StaleLease::Expired);
}

#[test]
fn terminal_commit_rechecks_the_durable_lease_after_fencing() {
    use sqlx::Row as _;

    let Some((mut durable, mut history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let command = TurnCommand::new(trust, None, "interrupt me").expect("valid command");
    let accepted = history
        .accept_initial(&command)
        .expect("initial acceptance");
    let runtime_state = RuntimeState::assemble();
    let harness = Harness::with_runtime(
        runtime_state.tool_execution_root().runtime(),
        tenant.clone(),
        accepted.thread_id,
        accepted.turn_id,
    );
    let (_authority, attempt) = harness.prepared();
    let binding = attempt.binding().clone();
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    // This models a C-6 recovery transaction winning after C-5 has read a
    // current lease but before its D-7 terminal write reaches PostgreSQL.
    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("fence lease after pre-check");
    });

    let cancelled = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Cancelled {
        effect_state: EffectState::NotStarted,
    });
    assert_eq!(
        durable.commit_terminal(&binding, &cancelled, 2_000),
        Ok(AttemptTerminalResolution::Fenced),
        "a fenced generation must not win the durable D-7 terminal transition",
    );
    let (status, version) = runtime.block_on(async {
        let row = sqlx::query(
            "SELECT status, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(tenant.as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("seeded attempt exists");
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<i64, _>("version").expect("version"),
        )
    });
    assert_eq!((status.as_str(), version), ("prepared", 1));
}

#[test]
fn dispatch_claim_rechecks_the_durable_lease_after_fencing() {
    let Some((mut durable, mut history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let accepted = history
        .accept_initial(&TurnCommand::new(trust, None, "claim fence").expect("valid command"))
        .expect("initial acceptance");
    let runtime_state = RuntimeState::assemble();
    let harness = Harness::with_runtime(
        runtime_state.tool_execution_root().runtime(),
        tenant.clone(),
        accepted.thread_id,
        accepted.turn_id,
    );
    let (_authority, attempt) = harness.prepared();
    let binding = attempt.binding().clone();
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("fence lease before dispatch claim");
    });

    assert_eq!(
        ExecutionAttemptStore::claim_running(&mut durable, &binding, 2_000),
        Ok(koduck_ai::application::DispatchClaimResolution::Fenced),
        "a fenced generation must not obtain durable dispatch authority",
    );
}

#[test]
fn committed_terminal_replays_even_after_lease_is_fenced() {
    let Some((mut durable, mut history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let command = TurnCommand::new(trust, None, "replay terminal").expect("valid command");
    let accepted = history
        .accept_initial(&command)
        .expect("initial acceptance");
    let runtime_state = RuntimeState::assemble();
    let harness = Harness::with_runtime(
        runtime_state.tool_execution_root().runtime(),
        tenant.clone(),
        accepted.thread_id,
        accepted.turn_id,
    );
    let (_authority, attempt) = harness.running(1_000);
    let binding = attempt.binding().clone();
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut durable, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );
    assert_eq!(
        ExecutionAttemptStore::claim_running(&mut durable, &binding, 2_000),
        Ok(koduck_ai::application::DispatchClaimResolution::Claimed { version: 2 })
    );
    let terminal = DurableAttemptTerminal::from_outcome(&ToolExecutionOutcome::Succeeded {
        output: b"committed".to_vec(),
        effect_state: EffectState::Started,
    });
    assert_eq!(
        durable.commit_terminal(&binding, &terminal, 3_000),
        Ok(AttemptTerminalResolution::Won { version: 3 })
    );

    runtime.block_on(async {
        sqlx::query(
            "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3",
        )
        .bind(tenant.as_str())
        .bind(accepted.thread_id.as_uuid())
        .bind(accepted.turn_id.as_uuid())
        .execute(&pool)
        .await
        .expect("fence lease after terminal commit");
    });

    assert!(matches!(
        durable.commit_terminal(&binding, &terminal, 4_000),
        Ok(AttemptTerminalResolution::ExistingTerminal(_))
    ));
}

/// A fenced or expired owner must not modify canonical state through the
/// interruption path (ADR-0003 TC-07): the C-5 interruption validates the real
/// durable lease generation before any D-7 mutation, so the request fails, the
/// prepared D-7 row keeps its exact state, and no Turn terminal is recorded.
fn stale_owner_interruption_mutates_nothing(stale: StaleLease) {
    use sqlx::Row as _;

    let Some((durable, mut history, pool, runtime)) = durable_backends() else {
        return;
    };
    let tenant = TenantId::new("tenant-a").expect("valid tenant");
    let trust =
        koduck_ai::domain::TrustContext::new(tenant.clone(), "subject-a").expect("valid principal");
    let command = TurnCommand::new(trust.clone(), None, "interrupt me").expect("valid command");
    let accepted = history
        .accept_initial(&command)
        .expect("initial acceptance");
    let runtime_state = RuntimeState::assemble();
    let harness = Harness::with_runtime(
        runtime_state.tool_execution_root().runtime(),
        tenant.clone(),
        accepted.thread_id,
        accepted.turn_id,
    );
    let (_authority, attempt) = harness.prepared();
    let binding: ExactActionBinding = attempt.binding().clone();
    let mut seeded = durable.clone();
    assert_eq!(
        ExecutionAttemptStore::insert_prepared(&mut seeded, &binding, 1_000),
        Ok(AttemptInsertResolution::Inserted),
    );

    // Take the lease away from the bound generation the way production does:
    // fencing mirrors the conditional recovery write (generation + 1, fenced),
    // expiry moves the lease outside the checked window.
    let statement = match stale {
        StaleLease::Fenced => {
            "UPDATE turn_leases SET fenced = TRUE, generation = generation + 1 \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3"
        }
        StaleLease::Expired => {
            "UPDATE turn_leases SET renewed_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes', \
             expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute' \
             WHERE tenant_id = $1 AND thread_id = $2 AND turn_id = $3"
        }
    };
    runtime.block_on(async {
        sqlx::query(statement)
            .bind(tenant.as_str())
            .bind(accepted.thread_id.as_uuid())
            .bind(accepted.turn_id.as_uuid())
            .execute(&pool)
            .await
            .expect("stale-lease setup");
    });

    let mut runner = TurnRunner::new(NoopProvider, history.clone()).with_tool_executor(
        runtime_state.tool_call_executor(
            durable.clone(),
            SqlxTurnLeaseValidator::new(pool.clone(), runtime.handle().clone()),
            koduck_ai::application::NoToolAudits,
            durable,
        ),
    );
    assert!(
        runner.request_interrupt(&trust, accepted.turn_id).is_err(),
        "a stale owner cannot interrupt through the C-5 cancellation boundary"
    );

    // The durable D-7 row carries no terminal: the cancellation never won a
    // conditional write for the fenced or expired generation.
    let (status, version) = runtime.block_on(async {
        let row = sqlx::query(
            "SELECT status, version FROM tool_execution_attempts \
             WHERE tenant_id = $1 AND attempt_id = $2",
        )
        .bind(tenant.as_str())
        .bind(binding.attempt_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("the seeded D-7 row still exists");
        (
            row.try_get::<String, _>("status").expect("status"),
            row.try_get::<i64, _>("version").expect("version"),
        )
    });
    assert_eq!(
        (status.as_str(), version),
        ("prepared", 1),
        "the stale-owner interruption left the prepared D-7 untouched"
    );

    // The Turn itself keeps no durable terminal either.
    let replayed = history
        .replay(&trust.tenant_id, accepted.turn_id)
        .expect("canonical replay");
    assert!(
        !replayed
            .iter()
            .any(|item| matches!(item.payload, ItemPayload::Terminal(_))),
        "the stale-owner interruption recorded no Turn terminal"
    );
}

/// Verifies the production `PostgreSQL` single-terminal arbitration on the
/// interrupted Turn: the durable interrupt request already committed the one
/// `Interrupted` terminal, so the late provider completion is rejected
/// `AlreadyTerminal` and the canonical replay returns exactly that terminal.
fn assert_single_arbitrated_turn_terminal(
    history: &mut PostgresTurnHistory<SqlxPostgresExecutor>,
    trust: &koduck_ai::domain::TrustContext,
    turn: &AcceptedTurn,
) {
    assert_eq!(
        history.append_provider_terminal(
            turn,
            TerminalOutcome::Completed {
                usage: Usage::new(1, 2).expect("valid usage"),
            },
        ),
        Err(HistoryError::AlreadyTerminal),
        "the durable interrupt request already committed the one Turn terminal"
    );
    let replayed = history
        .replay(&trust.tenant_id, turn.turn_id)
        .expect("canonical replay");
    let terminals: Vec<&ItemPayload> = replayed
        .iter()
        .map(|item| &item.payload)
        .filter(|payload| matches!(payload, ItemPayload::Terminal(_)))
        .collect();
    assert_eq!(
        terminals.len(),
        1,
        "replay contains exactly one durable Turn terminal"
    );
    assert_eq!(
        terminals[0],
        &ItemPayload::Terminal(TerminalOutcome::Interrupted)
    );
}
