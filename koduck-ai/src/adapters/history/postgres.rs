// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `PostgreSQL` history translation and exact foreground-lease policy.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::application::{
    AcceptedTurn, HistoryError, NewItem, TurnCommand, TurnHistory, TurnLiveness,
};
use crate::domain::{
    Item, LeaseGeneration, TenantId, TerminalOutcome, ThreadId, TrustContext, TurnId,
};

mod payload_codec;
mod recovery;
mod sqlx_executor;
#[cfg(test)]
mod tests;

pub use sqlx_executor::SqlxPostgresExecutor;

const MAX_BACKGROUND_WORKERS: usize = 256;

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

/// Adapter-owned operations required from a `PostgreSQL` transaction executor.
///
/// Implementations must bind every statement by tenant, Thread, Turn, and
/// generation and use the migration constraints shipped with this crate.
pub trait PostgresExecutor: Clone {
    /// Records an authenticated interrupt request conditionally on active ownership.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the Turn is not active and owned or storage fails.
    fn request_interrupt(&self, trust: &TrustContext, turn_id: TurnId) -> Result<(), HistoryError>;

    /// Reads the persisted interrupt flag for the expected generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when ownership is stale or storage fails.
    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError>;

    /// Reads tenant-scoped canonical Thread history in append order.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the Thread is not owned or storage fails.
    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError>;

    /// Atomically inserts initial Thread, Turn, input Item, and lease generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the transaction cannot commit.
    fn accept_initial(&self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError>;

    /// Conditionally allocates a sequence and appends under the expected generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when ownership is stale, terminal, or storage fails.
    fn append(&self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError>;

    /// Reads one tenant-scoped Turn in increasing sequence order.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the Turn is not owned or storage fails.
    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError>;

    /// Persists a renewal only for the current non-terminal generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when ownership is stale, terminal, or storage fails.
    fn renew_lease(&self, key: &LeaseKey, now_ms: u64) -> Result<(), HistoryError>;

    /// Atomically fences an eligible expired generation and appends its persisted-state terminal.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the key is stale, terminal, or storage fails.
    fn reconcile_expired(
        &self,
        key: &LeaseKey,
        now_ms: u64,
        timing: LeaseTiming,
    ) -> Result<ReconcileOutcome, HistoryError>;

    /// Lists active lease generations whose expiry and skew windows elapsed.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when canonical storage is unavailable.
    fn expired_lease_keys(
        &self,
        _now_ms: u64,
        _timing: LeaseTiming,
    ) -> Result<Vec<LeaseKey>, HistoryError> {
        Ok(Vec::new())
    }

    /// Advances an accepted append outage through `recovery-pending` to `failed`.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] while storage is unavailable or when ownership
    /// has been fenced or terminalized.
    fn recover_failed(
        &self,
        turn: &AcceptedTurn,
        timing: LeaseTiming,
    ) -> Result<RecoveryOutcome, HistoryError>;
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
}

impl TurnLiveness for LeaseRenewalGuard {
    fn stop_for_recovery(mut self: Box<Self>) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            if thread.join().is_err() {
                eprintln!("event=lease_renewal_join_failed error=worker-panicked");
            }
        }
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
}

impl<E: PostgresExecutor> PostgresTurnHistory<E> {
    /// Creates the `PostgreSQL` adapter with the exact approved lease timing.
    #[must_use]
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            timing: LeaseTiming::cand_1(),
            background: Arc::new(BackgroundAdmission::new(MAX_BACKGROUND_WORKERS)),
        }
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
        self.executor.reconcile_expired(key, now_ms, self.timing)
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
                                match executor.reconcile_expired(&key, now_ms, timing) {
                                    Ok(_)
                                    | Err(HistoryError::Fenced | HistoryError::AlreadyTerminal) => {
                                    }
                                    Err(error) => {
                                        eprintln!("event=lease_reconcile_failed error={error}");
                                    }
                                }
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
        let stop = Arc::new(AtomicBool::new(false));
        let renewal_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("koduck-ai-lease-renewal".to_owned())
            .spawn(move || {
                let _permit = permit;
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
            })
            .map_err(|_| HistoryError::Unavailable)?;
        Ok(Box::new(LeaseRenewalGuard {
            stop,
            thread: Some(thread),
        }))
    }

    fn request_interrupt(
        &mut self,
        trust: &TrustContext,
        turn_id: TurnId,
    ) -> Result<(), HistoryError> {
        self.executor.request_interrupt(trust, turn_id)
    }

    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError> {
        self.executor.interruption_requested(turn)
    }

    fn prior_thread_items(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        self.executor.prior_thread_items(trust, thread_id)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        self.executor.accept_initial(command)
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.executor.append(turn, item)
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
        )
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
