// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! `PostgreSQL` history translation and exact foreground-lease policy.

use crate::application::{AcceptedTurn, HistoryError, NewItem, TurnCommand, TurnHistory};
use crate::domain::{Item, LeaseGeneration, TenantId, ThreadId, TrustContext, TurnId};

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
    /// This reconciler fenced the generation and appended `cancelled`.
    Cancelled,
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
        tenant_id: &TenantId,
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

    /// Atomically fences an eligible expired generation and appends one cancellation.
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
}

/// The sole production canonical-history implementation for CAND-1.
#[derive(Clone)]
pub struct PostgresTurnHistory<E> {
    executor: E,
    timing: LeaseTiming,
}

impl<E: PostgresExecutor> PostgresTurnHistory<E> {
    /// Creates the `PostgreSQL` adapter with the exact approved lease timing.
    #[must_use]
    pub const fn new(executor: E) -> Self {
        Self {
            executor,
            timing: LeaseTiming::cand_1(),
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

    /// Conditionally fences an expired generation and appends one cancellation.
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
}

impl<E: PostgresExecutor> TurnHistory for PostgresTurnHistory<E> {
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
        tenant_id: &TenantId,
        thread_id: ThreadId,
    ) -> Result<Vec<Item>, HistoryError> {
        self.executor.prior_thread_items(tenant_id, thread_id)
    }

    fn accept_initial(&mut self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError> {
        self.executor.accept_initial(command)
    }

    fn append(&mut self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError> {
        self.executor.append(turn, item)
    }

    fn replay(&self, tenant_id: &TenantId, turn_id: TurnId) -> Result<Vec<Item>, HistoryError> {
        self.executor.replay(tenant_id, turn_id)
    }
}
