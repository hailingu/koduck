// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Consumer-owned port for canonical D-7 execution-attempt persistence.

use thiserror::Error;

use crate::domain::execution::{ExactActionBinding, ExecutionStatus};
use crate::domain::{TenantId, ThreadId, TurnId};

use super::execution::{CanonicalAttemptTerminal, ToolExecutionOutcome};
use super::executor_envelope::{EffectState, ExecutionFailure};

/// A canonical D-7 store operation could not complete or was rejected.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AttemptStoreError {
    /// The durable store did not complete within its availability contract.
    #[error("execution attempt store unavailable")]
    Unavailable,
    /// The canonical identity exists with different immutable fields.
    ///
    /// A replay whose binding no longer matches the committed record can
    /// never be reconciled as the same attempt and must not overwrite it.
    #[error("execution attempt identity conflicts with the canonical record")]
    IdentityConflict,
}

/// The outcome of one durable idempotent D-7 insert.
///
/// A replay of the same immutable record — including after a lost
/// acknowledgement whose write actually committed — reports
/// [`AttemptInsertResolution::Existing`] after verifying every immutable
/// field and returning the row's current canonical projection, so the caller
/// reconciles unambiguously instead of retrying blind (ADR-0003 TC-12).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptInsertResolution {
    /// This call committed the prepared record at version 1.
    Inserted,
    /// The identical canonical record already exists; no state changed, and
    /// the row's current canonical projection is returned.
    Existing {
        /// The canonical status at replay time.
        status: ExecutionStatus,
        /// The canonical record version at replay time.
        version: u64,
    },
}

/// The outcome of one durable conditional D-7 dispatch claim.
///
/// Exactly one contender observes [`DispatchClaimResolution::Claimed`]; every
/// other contender of the same attempt observes the canonical state through
/// [`DispatchClaimResolution::Existing`], and a prepared attempt of a Turn
/// whose single running slot is owned by another D-7 observes
/// [`DispatchClaimResolution::Concurrent`] with no state change (TC-09/TC-12).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchClaimResolution {
    /// This caller's conditional `prepared -> running` transition won and is
    /// now canonical at version 2.
    Claimed {
        /// The canonical record version after the transition.
        version: u64,
    },
    /// Another contender's transition already won; this caller changed no
    /// state and observes the row's canonical status and version.
    Existing {
        /// The canonical status at read time.
        status: ExecutionStatus,
        /// The canonical record version at read time.
        version: u64,
    },
    /// Another D-7 owns this Turn's single running slot, so this prepared
    /// attempt cannot claim a dispatch.
    Concurrent,
    /// The bound lease is missing, fenced, superseded, or expired, so this
    /// caller cannot obtain durable dispatch authority.
    Fenced,
    /// No canonical D-7 exists for this identity in this tenant.
    NotFound,
}

/// One canonical D-7 terminal to be durably committed.
///
/// The terminal carries only bounded canonical evidence: the terminal
/// status, the executor-observed effect state, the stable failure code for a
/// failed terminal, and the bounded committed output for a success. It is
/// derived from one [`ToolExecutionOutcome`] so the durable row and the C-5
/// outcome cannot drift (ADR-0003 TC-12).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableAttemptTerminal {
    status: ExecutionStatus,
    effect_state: EffectState,
    failure_code: Option<ExecutionFailure>,
    output: Option<Vec<u8>>,
}

impl DurableAttemptTerminal {
    /// Derives the canonical terminal from one C-5 execution outcome.
    #[must_use]
    pub fn from_outcome(outcome: &ToolExecutionOutcome) -> Self {
        match outcome {
            ToolExecutionOutcome::Succeeded {
                output,
                effect_state,
            } => Self {
                status: ExecutionStatus::Succeeded,
                effect_state: *effect_state,
                failure_code: None,
                output: Some(output.clone()),
            },
            ToolExecutionOutcome::Cancelled { effect_state } => Self {
                status: ExecutionStatus::Cancelled,
                effect_state: *effect_state,
                failure_code: None,
                output: None,
            },
            ToolExecutionOutcome::TimedOut { effect_state } => Self {
                status: ExecutionStatus::TimedOut,
                effect_state: *effect_state,
                failure_code: None,
                output: None,
            },
            ToolExecutionOutcome::Failed { code, effect_state } => Self {
                status: ExecutionStatus::Failed,
                effect_state: *effect_state,
                failure_code: Some(*code),
                output: None,
            },
        }
    }

    /// Returns the canonical terminal status.
    #[must_use]
    pub const fn status(&self) -> ExecutionStatus {
        self.status
    }

    /// Returns the executor-observed effect state recorded with the terminal.
    #[must_use]
    pub const fn effect_state(&self) -> EffectState {
        self.effect_state
    }

    /// Returns the stable failure code of a failed terminal.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ExecutionFailure> {
        self.failure_code
    }

    /// Returns the bounded committed output of a success terminal.
    #[must_use]
    pub fn output(&self) -> Option<&[u8]> {
        self.output.as_deref()
    }

    /// Returns whether this terminal may commit from the recorded status.
    ///
    /// A cancellation may close a still-prepared attempt (a declined,
    /// cancelled, or expired D-6) only when the executor proves no effect
    /// started; a cancellation reporting started or unknown effect evidence
    /// and every success, failure, and timeout terminal require the won
    /// dispatch claim (ADR-0003 D-7 transitions).
    #[must_use]
    pub fn legal_from(&self, status: ExecutionStatus) -> bool {
        match self.status {
            ExecutionStatus::Cancelled => match status {
                ExecutionStatus::Prepared => self.effect_state == EffectState::NotStarted,
                ExecutionStatus::Running => true,
                _ => false,
            },
            ExecutionStatus::Succeeded | ExecutionStatus::Failed | ExecutionStatus::TimedOut => {
                matches!(status, ExecutionStatus::Running)
            }
            ExecutionStatus::Prepared | ExecutionStatus::Running => false,
        }
    }
}

/// The outcome of one durable conditional D-7 terminal commit.
///
/// Exactly one contender observes [`AttemptTerminalResolution::Won`]; every
/// other contender observes the already-committed canonical terminal through
/// [`AttemptTerminalResolution::ExistingTerminal`]. A terminal transition
/// whose bound lease is fenced, superseded, or expired returns
/// [`AttemptTerminalResolution::Fenced`], and one that is not legal from the
/// canonical state is a typed conflict that changes no state (ADR-0003 TC-12).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttemptTerminalResolution {
    /// This caller's conditional transition won and is now canonical at
    /// version 3.
    Won {
        /// The canonical record version after the transition.
        version: u64,
    },
    /// A terminal already won canonically; this caller changed no state and
    /// receives the validated canonical terminal for reconciliation.
    ExistingTerminal(Box<CanonicalAttemptTerminal>),
    /// The transition is not legal from the canonical state (for example a
    /// success terminal for a still-prepared attempt, or a durable row whose
    /// immutable binding no longer matches); reconciliation owns the next
    /// transition.
    Conflict,
    /// The bound lease was fenced, superseded, or expired before this terminal
    /// transition could commit; no D-7 state changed.
    Fenced,
    /// No canonical D-7 exists for this identity in this tenant.
    NotFound,
}

/// Consumer-owned boundary for durable canonical D-7 records.
///
/// The store is the only cross-instance authority for D-7 state: the insert,
/// the dispatch claim, and the terminal commit are conditional durable
/// writes, so competing dispatchers, terminal results, and reconcilers
/// converge on one canonical outcome with exactly one running D-7 per Turn
/// (ADR-0003 TC-09/TC-12).
pub trait ExecutionAttemptStore {
    /// Durably inserts one newly prepared D-7, idempotently.
    ///
    /// A replay of the same immutable record — including after a lost
    /// acknowledgement whose write actually committed — reports
    /// [`AttemptInsertResolution::Existing`] after verifying every immutable
    /// field, so the canonical row can be reconciled without ambiguity. The
    /// same identity with different immutable fields is a typed
    /// [`AttemptStoreError::IdentityConflict`] that changes no state.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptStoreError::Unavailable`] when the durable write
    /// cannot complete within its availability contract, or
    /// [`AttemptStoreError::IdentityConflict`] when the identity exists with
    /// different immutable fields.
    fn insert_prepared(
        &mut self,
        binding: &ExactActionBinding,
        prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError>;

    /// Claims the Turn's only running slot for one prepared D-7.
    ///
    /// The conditional `prepared -> running` update permits exactly one
    /// winner and binds the full immutable record, so an attempt identity
    /// replayed with drifted Thread, Turn, lease, action, or profile fields
    /// can never claim another canonical D-7. The durable boundary keeps at
    /// most one running D-7 per Turn, so a claim for a Turn whose slot is
    /// owned by another attempt reports
    /// [`DispatchClaimResolution::Concurrent`] with no state change. A missing,
    /// fenced, superseded, or expired bound lease reports
    /// [`DispatchClaimResolution::Fenced`] instead of authorizing dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptStoreError::Unavailable`] when the durable
    /// transition cannot complete within its availability contract, or
    /// [`AttemptStoreError::IdentityConflict`] when the identity exists with
    /// different immutable fields.
    fn claim_running(
        &mut self,
        binding: &ExactActionBinding,
        started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError>;

    /// Commits one bounded terminal through a conditional transition.
    ///
    /// A cancellation may close a running attempt, or a still-prepared
    /// attempt when the executor proves no effect started; a success,
    /// failure, or timeout terminal requires the won dispatch claim. The
    /// conditional update binds the full immutable record, so an attempt
    /// identity replayed with drifted fields can never terminalize another
    /// canonical D-7. The conditional write requires the bound C-6 lease to
    /// exist and be current. Exactly one conditional write wins; every other
    /// contender observes the committed canonical terminal through
    /// [`AttemptTerminalResolution::ExistingTerminal`] or a typed fence.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptStoreError::Unavailable`] when the durable
    /// transition cannot complete within its availability contract.
    fn commit_terminal(
        &mut self,
        binding: &ExactActionBinding,
        terminal: &DurableAttemptTerminal,
        terminal_at_millis: u64,
    ) -> Result<AttemptTerminalResolution, AttemptStoreError>;
}

/// Consumer-owned lookup of live canonical D-7 work for one authenticated Turn.
///
/// A process-local execution catalog can only report work it has registered
/// itself. The runner uses this durable lookup before accepting a local
/// `NoLiveAttempt` interruption outcome, so an already-live remote owner or
/// restarted process is reconciled rather than hidden (ADR-0003 TC-10).
pub trait ExecutionAttemptLiveness {
    /// Reports whether the canonical store has a prepared or running D-7 for
    /// this exact tenant, Thread, and Turn.
    ///
    /// # Errors
    ///
    /// Returns [`AttemptStoreError::Unavailable`] when the durable answer
    /// cannot be obtained within its availability contract.
    fn has_live_attempt(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<bool, AttemptStoreError>;
}

/// Consumer-owned durable barrier that blocks new D-7 dispatch for an
/// authenticated Turn interruption.
///
/// The barrier is committed before C-5 inspects or cancels process-local
/// attempts. Every durable preparation and dispatch claim checks the same
/// barrier under the Turn ownership lock, so a remote instance cannot create
/// new external work between a local no-live observation and the final Turn
/// interruption terminal (ADR-0003 TC-10/TC-12).
pub trait ExecutionAttemptInterruptionGuard {
    /// Prevents new prepared or running D-7 transitions for this Turn.
    ///
    /// # Errors
    ///
    /// Returns the unavailable store error when the durable barrier cannot be
    /// committed while the Turn has a current lease.
    fn begin_interruption(
        &mut self,
        tenant_id: &TenantId,
        thread_id: ThreadId,
        turn_id: TurnId,
    ) -> Result<(), AttemptStoreError>;
}
