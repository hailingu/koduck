// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md
// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md
// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! `PostgreSQL` history translation and exact foreground-lease policy.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::application::{
    AcceptedTurn, HistoryError, NewItem, PriorTurnHistory, RecoveryHandoff, TurnCommand,
    TurnHistory, TurnLiveness,
};
use crate::domain::{
    Item, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
};

mod approval_terminal_backfill;
mod attempt_recovery;
mod commit_reconciliation;
mod executor_port;
mod payload_codec;
mod recovery;
mod sqlx_executor;
#[cfg(test)]
mod tests;

pub use executor_port::PostgresExecutor;
pub use payload_codec::{DurableItemCodec, DurableItemColumns};
pub use sqlx_executor::SqlxPostgresExecutor;

const MAX_BACKGROUND_WORKERS: usize = 256;

async fn settle_commit_attempt<T, O, R>(
    deadline: Duration,
    operation: O,
    reconcile: R,
) -> Result<T, HistoryError>
where
    O: Future<Output = Result<T, HistoryError>>,
    R: Future<Output = Result<Option<T>, HistoryError>>,
{
    match tokio::time::timeout(deadline, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(HistoryError::Unavailable)) | Err(_) => tokio::time::timeout(deadline, reconcile)
            .await
            .map_err(|_| HistoryError::Unavailable)??
            .ok_or(HistoryError::Unavailable),
        Ok(Err(error)) => Err(error),
    }
}

struct BackgroundAdmission {
    active: AtomicUsize,
    limit: usize,
}

impl BackgroundAdmission {
    const fn new(limit: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            limit,
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Result<BackgroundPermit, HistoryError> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.limit).then_some(active + 1)
            })
            .map_err(|_| HistoryError::Unavailable)?;
        Ok(BackgroundPermit(Arc::clone(self)))
    }
}

struct BackgroundPermit(Arc<BackgroundAdmission>);

impl Drop for BackgroundPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Complete conditional key for one foreground lease generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseKey {
    /// Tenant that owns the Thread and Turn.
    pub tenant_id: TenantId,
    /// AI-owned Thread identity.
    pub thread_id: ThreadId,
    /// Immutable Turn attempt identity.
    pub turn_id: TurnId,
    /// Expected foreground owner generation.
    pub generation: LeaseGeneration,
}

impl LeaseKey {
    /// Creates the complete tenant/Thread/Turn/generation conditional key.
    #[must_use]
    pub const fn new(
        tenant_id: TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
        generation: LeaseGeneration,
    ) -> Self {
        Self {
            tenant_id,
            thread_id,
            turn_id,
            generation,
        }
    }

    /// Checks whether a persisted tenant and accepted Turn equal this full key.
    #[must_use]
    pub fn matches(&self, tenant_id: &TenantId, turn: &AcceptedTurn) -> bool {
        &self.tenant_id == tenant_id
            && self.thread_id == turn.thread_id
            && self.turn_id == turn.turn_id
            && self.generation == turn.generation
    }
}

/// Exact CAND-1 heartbeat, lease, and clock-skew windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseTiming {
    heartbeat: u64,
    lease: u64,
    clock_skew: u64,
}

impl LeaseTiming {
    /// Returns the approved 5-second heartbeat, 20-second lease, and 2-second skew.
    #[must_use]
    pub const fn cand_1() -> Self {
        Self {
            heartbeat: 5_000,
            lease: 20_000,
            clock_skew: 2_000,
        }
    }

    /// Returns the interval between persisted foreground renewals.
    #[must_use]
    pub const fn heartbeat_ms(self) -> u64 {
        self.heartbeat
    }

    /// Returns the elapsed time after the last renewal when reconciliation is eligible.
    #[must_use]
    pub const fn reconcile_after_ms(self) -> u64 {
        self.lease + self.clock_skew
    }
}

/// Result of one conditional expired-owner reconciliation attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconcileOutcome {
    /// Lease has not passed its 20-second expiry plus 2-second skew margin.
    TooEarly,
    /// This reconciler fenced an active orphan and appended `cancelled`.
    Cancelled,
    /// This reconciler finished a durability recovery with `failed`.
    Failed,
    /// This reconciler preserved an accepted interrupt as `interrupted`.
    Interrupted,
}

/// Progress made by one conditional durability-recovery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    /// The owned turn is durably `recovery-pending` and needs a terminal append.
    Pending,
    /// The owned turn is durably closed as `failed`.
    Failed,
}

/// Receives a best-effort notification after a canonical Turn terminal.
///
/// The observer owns no lifecycle authority: it must independently prove the
/// terminal before releasing any process-local resource.
pub trait TurnTerminalObserver: Send + Sync {
    /// Observes a Turn whose terminal may have been durably committed.
    fn terminal_may_have_committed(
        &self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    );
}

/// Default observer for histories without process-local terminal resources.
#[derive(Default)]
pub struct NoTurnTerminalObserver;

impl TurnTerminalObserver for NoTurnTerminalObserver {
    fn terminal_may_have_committed(
        &self,
        _tenant_id: &TenantId,
        _thread_id: ThreadId,
        _turn_id: TurnId,
    ) {
    }
}

/// A background orphan-reconciliation worker stopped when dropped.
pub struct ReconciliationWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ReconciliationWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            // A scan may be blocked inside a degraded database call. Dropping
            // the handle lets runtime shutdown return while the owned worker
            // observes `stop` as soon as that call completes.
        }
    }
}

struct LeaseRenewalGuard {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    permit_receiver: Option<mpsc::Receiver<BackgroundPermit>>,
    recovery: Option<RecoveryStarter>,
}

type RecoveryStarter =
    Box<dyn FnOnce(BackgroundPermit) -> Result<(), HistoryError> + Send + 'static>;

impl TurnLiveness for LeaseRenewalGuard {
    fn handoff_to_recovery(mut self: Box<Self>) -> Result<RecoveryHandoff, HistoryError> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            if thread.join().is_err() {
                eprintln!("event=lease_renewal_join_failed error=worker-panicked");
                return Err(HistoryError::Unavailable);
            }
        }
        let permit = self
            .permit_receiver
            .take()
            .ok_or(HistoryError::Unavailable)?
            .recv()
            .map_err(|_| HistoryError::Unavailable)?;
        self.recovery.take().ok_or(HistoryError::Unavailable)?(permit)?;
        Ok(RecoveryHandoff::Recovered)
    }
}

impl Drop for LeaseRenewalGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            // A renewal may be blocked inside a degraded database call. Dropping
            // the handle lets request shutdown return while the owned worker
            // observes `stop` as soon as that call completes.
        }
    }
}

/// The sole production canonical-history implementation for CAND-1.
#[derive(Clone)]
pub struct PostgresTurnHistory<E> {
    executor: E,
    timing: LeaseTiming,
    background: Arc<BackgroundAdmission>,
    terminal_observer: Arc<dyn TurnTerminalObserver>,
}

impl<E: PostgresExecutor> PostgresTurnHistory<E> {
    /// Creates the `PostgreSQL` adapter with the exact approved lease timing.
    #[must_use]
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            timing: LeaseTiming::cand_1(),
            background: Arc::new(BackgroundAdmission::new(MAX_BACKGROUND_WORKERS)),
            terminal_observer: Arc::new(NoTurnTerminalObserver),
        }
    }

    /// Returns this history with a terminal observer for background completions.
    #[must_use]
    pub fn with_terminal_observer(mut self, observer: Arc<dyn TurnTerminalObserver>) -> Self {
        self.terminal_observer = observer;
        self
    }

    /// Persists a foreground heartbeat for the complete expected lease key.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when storage is unavailable or ownership is stale.
    pub fn renew_lease(&mut self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError> {
        self.executor.renew_lease(key, now_ms)
    }

    /// Conditionally fences an expired generation and appends its persisted-state terminal.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when storage is unavailable, another reconciler
    /// has already terminated the Turn, or the complete key is stale.
    pub fn reconcile_expired(
        &mut self,
        key: &LeaseKey,
        now_ms: u64,
    ) -> Result<ReconcileOutcome, HistoryError> {
        reconcile_and_notify(
            &self.executor,
            key,
            now_ms,
            self.timing,
            &self.terminal_observer,
        )
    }

    /// Starts the production loop that fences orphaned expired generations.
    ///
    /// # Errors
    ///
    /// Returns the operating-system spawn error when the worker thread cannot start.
    pub fn start_reconciliation_worker(&self) -> Result<ReconciliationWorker, std::io::Error>
    where
        E: Send + 'static,
    {
        let executor = self.executor.clone();
        let timing = self.timing;
        let terminal_observer = Arc::clone(&self.terminal_observer);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("koduck-ai-reconciliation".to_owned())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let now_ms = unix_time_ms();
                    match executor.expired_lease_keys(now_ms, timing) {
                        Ok(keys) => {
                            for key in keys {
                                let _ = reconcile_and_notify(
                                    &executor,
                                    &key,
                                    now_ms,
                                    timing,
                                    &terminal_observer,
                                );
                            }
                        }
                        Err(error) => eprintln!("event=lease_scan_failed error={error}"),
                    }
                    thread::park_timeout(Duration::from_millis(timing.heartbeat_ms()));
                }
            })?;
        Ok(ReconciliationWorker {
            stop,
            thread: Some(thread),
        })
    }
}

/// Reconciles one expired generation and notifies the terminal observer for
/// every outcome that may have durably terminalized the Turn.
///
/// Both the background worker and the public `PostgresTurnHistory::reconcile_expired`
/// entry route through this helper, so a caller that terminalizes a Turn
/// directly cannot leave its configured C-5 authority retained. The observer
/// independently proves the terminal before releasing anything, so notifying
/// on a lost race (`AlreadyTerminal`) stays safe.
fn reconcile_and_notify<E: PostgresExecutor>(
    executor: &E,
    key: &LeaseKey,
    now_ms: u64,
    timing: LeaseTiming,
    terminal_observer: &Arc<dyn TurnTerminalObserver>,
) -> Result<ReconcileOutcome, HistoryError> {
    let outcome = executor.reconcile_expired(key, now_ms, timing);
    match &outcome {
        Ok(
            ReconcileOutcome::Cancelled
            | ReconcileOutcome::Failed
            | ReconcileOutcome::Interrupted,
        )
        // Another reconciler may have committed the terminal after this
        // caller listed the expired key.
        | Err(HistoryError::AlreadyTerminal) => terminal_observer.terminal_may_have_committed(
            &key.tenant_id,
            key.thread_id,
            key.turn_id,
        ),
        Ok(ReconcileOutcome::TooEarly) | Err(HistoryError::Fenced) => {}
        Err(error) => {
            eprintln!("event=lease_reconcile_failed error={error}");
        }
    }
    outcome
}

impl<E: PostgresExecutor + Send + 'static> TurnHistory for PostgresTurnHistory<E> {
    fn start_turn_liveness(
        &self,
        turn: &AcceptedTurn,
    ) -> Result<Box<dyn TurnLiveness>, HistoryError> {
        let executor = self.executor.clone();
        let key = LeaseKey::new(
            turn.tenant_id.clone(),
            turn.thread_id,
            turn.turn_id,
            turn.generation,
        );
        let heartbeat = self.timing.heartbeat_ms();
        let permit = self.background.try_acquire()?;
        let (permit_sender, permit_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let renewal_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("koduck-ai-lease-renewal".to_owned())
            .spawn(move || {
                while !renewal_stop.load(Ordering::Acquire) {
                    thread::park_timeout(Duration::from_millis(heartbeat));
                    if renewal_stop.load(Ordering::Acquire) {
                        break;
                    }
                    match executor.renew_lease(&key, unix_time_ms()) {
                        Ok(()) => {}
                        Err(HistoryError::Unavailable) => {
                            eprintln!("event=lease_renewal_retry error=durability-unavailable");
                        }
                        Err(error) => {
                            eprintln!("event=lease_renewal_stopped error={error}");
                            break;
                        }
                    }
                }
                let _ = permit_sender.send(permit);
            })
            .map_err(|_| HistoryError::Unavailable)?;
        let recovery_executor = self.executor.clone();
        let recovery_turn = turn.clone();
        let recovery_timing = self.timing;
        Ok(Box::new(LeaseRenewalGuard {
            stop,
            thread: Some(thread),
            permit_receiver: Some(permit_receiver),
            // The in-request handoff deliberately skips the history-side
            // terminal observer: the runner owns this recovery and notifies
            // the tool boundary itself after the Recovered result, so a
            // second observer probe would duplicate the durable query and
            // reclamation work. Recovery the runner does not own — the
            // background worker and scheduled jobs — keeps notifying through
            // this history's observer.
            recovery: Some(Box::new(move |permit| {
                recovery::recover_with_permit(
                    &recovery_executor,
                    &recovery_turn,
                    recovery_timing,
                    permit,
                );
                Ok(())
            })),
        }))
    }

    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
        tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError> {
        self.executor
            .request_interrupt(trust, turn_id, tool_terminals)
    }

    fn interruption_thread(
        &self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<Option<ThreadId>, HistoryError> {
        self.executor.interruption_thread(trust, turn_id)
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        self.executor.interruption_requested(turn)
    }

    fn prior_thread_turns(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        self.executor.prior_thread_turns(trust, thread_id)
    }

    fn prior_thread_turns_bounded(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
        deadline: Duration,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError> {
        // SI-07c: the history read's two-second maximum is absolute; a
        // direct caller's longer deadline cannot extend it.
        let deadline = deadline.min(crate::application::LOOKUP_BUDGET);
        self.executor
            .prior_thread_turns_bounded(trust, thread_id, deadline)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        if command.submission_id.is_some() {
            return Err(HistoryError::Unavailable);
        }
        self.executor.accept_initial(command)
    }

    fn submission_observation(
        &self,
        command: &TurnCommand,
        deadline: Duration,
    ) -> Result<Option<crate::application::SubmissionObservation>, HistoryError> {
        // SI-01d: the wrapper validates before delegation so a custom
        // executor cannot receive an invalid command's database I/O.
        crate::application::validate_identified_command(command)
            .map_err(|_| HistoryError::Unavailable)?;
        // SI-07c: the lookup's two-second maximum is absolute; a direct
        // caller's longer deadline cannot extend it.
        let deadline = deadline.min(crate::application::LOOKUP_BUDGET);
        self.executor.submission_observation(command, deadline)
    }

    fn accept_initial_with_submission(
        &mut self,
        command: &TurnCommand,
        deadline: std::time::Duration,
    ) -> Result<crate::application::IdentifiedAcceptance, HistoryError> {
        // SI-01d: the wrapper validates before delegation so a custom
        // executor cannot receive an invalid command's database I/O.
        crate::application::validate_identified_command(command)
            .map_err(|_| HistoryError::Unavailable)?;
        // SI-07c: the write attempt's reserved budget is fixed at two
        // seconds; a direct caller's longer budget cannot extend it.
        let deadline = deadline.min(crate::application::WRITE_BUDGET);
        self.executor
            .accept_initial_with_submission(command, deadline)
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.executor.append(turn, item)
    }

    fn append_tool_projection(
        &mut self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        self.executor.append_tool_projection(turn, items)
    }

    fn append_provider_terminal(
        &mut self,
        turn: &AcceptedTurn,
        outcome: TerminalOutcome,
    ) -> Result<Item, HistoryError> {
        self.executor.append(turn, NewItem::Terminal(outcome))
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.executor.replay(tenant_id, turn_id)
    }

    fn schedule_failed_recovery(&mut self, turn: &AcceptedTurn) -> Result<(), HistoryError> {
        recovery::schedule(
            self.executor.clone(),
            turn.clone(),
            self.timing,
            &self.background,
            Arc::clone(&self.terminal_observer),
        )
    }
}

/// Wall-clock epoch milliseconds shared by the runtime's synchronous adapters.
pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
