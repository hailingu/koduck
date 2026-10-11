// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md
// ADR: koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md
// ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md

//! The consumer-owned port every `PostgreSQL` transaction executor must
//! implement, with the bounded identified-submission entries this slice
//! added (ADR-0018 SI-01d, SI-03, SI-06, SI-07c).

use std::time::Duration;

use crate::application::{AcceptedTurn, HistoryError, NewItem, PriorTurnHistory, TurnCommand};
use crate::domain::{Item, TenantId, ThreadId, TrustContext, TurnId};

use super::{LeaseKey, LeaseTiming, ReconcileOutcome, RecoveryOutcome};

/// Adapter-owned operations required from a `PostgreSQL` transaction executor.
///
/// Implementations must bind every statement by tenant, Thread, Turn, and
/// generation and use the migration constraints shipped with this crate.
pub trait PostgresExecutor: Clone {
    /// Atomically appends D-7 terminal items and the authenticated Turn
    /// interruption terminal, conditionally on active ownership.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the Turn is not active and owned or storage fails.
    fn request_interrupt(
        &self,
        trust: &TrustContext,
        turn_id: TurnId,
        tool_terminals: Vec<NewItem>,
    ) -> Result<(), HistoryError>;

    /// Resolves the owned Thread for a paired C-5 interruption.
    ///
    /// Adapters without a colocated C-5 boundary may return `None`; the
    /// production `SQLx` adapter returns the authenticated Turn's Thread.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when resolving the authenticated ownership
    /// context cannot complete.
    fn interruption_thread(
        &self,
        _trust: &TrustContext,
        _turn_id: TurnId,
    ) -> Result<Option<ThreadId>, HistoryError> {
        Ok(None)
    }

    /// Reads the persisted interrupt flag for the expected generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when ownership is stale or storage fails.
    fn interruption_requested(&self, turn: &AcceptedTurn) -> Result<bool, HistoryError>;

    /// Reads the complete bounded prior source-Turn groups of a subject-owned
    /// Thread in canonical order with actual per-row provenance
    /// (ADR-0006 PC-01).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the Thread is not owned, storage fails,
    /// a row cannot be decoded, or the canonical provider context exceeds its
    /// aggregate budget.
    fn prior_thread_turns(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError>;

    /// Reads the bounded prior history within the caller-owned deadline.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the bounded read fails.
    fn prior_thread_turns_bounded(
        &self,
        trust: &TrustContext,
        thread_id: ThreadId,
        deadline: Duration,
    ) -> Result<Vec<PriorTurnHistory>, HistoryError>;

    /// Atomically inserts initial Thread, Turn, input Item, and lease generation.
    /// Rejects commands carrying a submission ID before database I/O; use
    /// [`Self::accept_initial_with_submission`] for identified acceptance.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError::Unavailable`] for an identified command, or
    /// [`HistoryError`] when the transaction cannot commit.
    fn accept_initial(&self, command: &TurnCommand) -> Result<AcceptedTurn, HistoryError>;

    /// Reads one owned submission binding unlocked, validating its joined
    /// canonical structure against the candidate command (ADR-0018 SI-07a,
    /// SI-08c).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] for unavailability, corrupt structure, or the
    /// typed drift conflict.
    fn submission_observation(
        &self,
        command: &TurnCommand,
        deadline: Duration,
    ) -> Result<Option<crate::application::SubmissionObservation>, HistoryError>;

    /// Atomically rechecks and accepts one identified submission with its
    /// binding and bounded settlement (ADR-0018 SI-03a, SI-06, SI-07c).
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the attempt cannot commit or the key
    /// drifted.
    fn accept_initial_with_submission(
        &self,
        command: &TurnCommand,
        attempt_budget: Duration,
    ) -> Result<crate::application::IdentifiedAcceptance, HistoryError>;

    /// Conditionally allocates a sequence and appends under the expected generation.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when ownership is stale, terminal, or storage fails.
    fn append(&self, turn: &AcceptedTurn, item: NewItem) -> Result<Item, HistoryError>;

    /// Atomically appends the complete D-3 sequence for one Tool projection.
    ///
    /// Implementations MUST commit every item in sequence order or none. The
    /// default rejects multi-item sequences rather than risking a partial
    /// durable projection in adapters that have not implemented a transaction.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the sequence cannot be committed as one
    /// operation under the expected lease generation.
    fn append_tool_projection(
        &self,
        turn: &AcceptedTurn,
        items: Vec<NewItem>,
    ) -> Result<Vec<Item>, HistoryError> {
        if items.len() != 1 {
            return Err(HistoryError::Unavailable);
        }
        self.append(
            turn,
            items
                .into_iter()
                .next()
                .expect("one checked projection item exists"),
        )
        .map(|item| vec![item])
    }

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

    /// Advances recovery within one caller-owned attempt budget.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryError`] when the bounded attempt cannot determine an outcome.
    fn recover_failed_with_deadline(
        &self,
        turn: &AcceptedTurn,
        timing: LeaseTiming,
        _deadline: Duration,
    ) -> Result<RecoveryOutcome, HistoryError> {
        self.recover_failed(turn, timing)
    }
}
