// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime-owned lookup and strong retention for Turn execution authority.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::{
    AttemptBudget, ExactActionBinding, ExecutionAttempt, ExecutionStatus, TurnAuthorityState,
    TurnExecutionAuthority, recover_lock,
};
use crate::domain::{TenantId, ThreadId, TurnId};

/// Sole in-memory owner of one Turn's D-7 allocation budget key.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct TurnAuthorityKey {
    pub(super) tenant: TenantId,
    pub(super) thread: ThreadId,
    pub(super) turn: TurnId,
}

/// Runtime-owned catalog that shares one live authority across preparers.
#[derive(Debug, Default)]
pub(crate) struct TurnAuthorityCatalog {
    state: Mutex<TurnAuthorityCatalogState>,
}

/// Catalog state coupling live authorities with interruption tombstones.
#[derive(Debug, Default)]
struct TurnAuthorityCatalogState {
    states: HashMap<TurnAuthorityKey, Arc<Mutex<TurnAuthorityState>>>,
    interrupted: HashSet<TurnAuthorityKey>,
}

impl TurnAuthorityCatalog {
    pub(crate) fn authority_for(&self, binding: &ExactActionBinding) -> TurnExecutionAuthority {
        let key = key_for(binding);
        let mut catalog = recover_lock(&self.state);
        if let Some(state) = catalog.states.get(&key) {
            return TurnExecutionAuthority {
                state: Arc::clone(state),
            };
        }
        let interruption_requested = catalog.interrupted.contains(&key);
        let state = Arc::new(Mutex::new(TurnAuthorityState {
            key: key.clone(),
            profile_id: binding.profile_id.clone(),
            profile_version: binding.profile_version.clone(),
            budget: AttemptBudget::new(),
            attempts: BTreeMap::new(),
            interruption_requested,
        }));
        catalog.states.insert(key, Arc::clone(&state));
        TurnExecutionAuthority { state }
    }

    /// Seals one identified Turn against future allocation and returns its authority.
    ///
    /// The catalog lock serializes tombstone creation with first authority
    /// creation. The authority lock then serializes the interruption flag with
    /// D-7 allocation, so an allocation either appears in the interruption
    /// snapshot or is rejected after the Turn has been sealed.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn request_interruption(
        &self,
        tenant: &TenantId,
        thread: ThreadId,
        turn: TurnId,
    ) -> Option<TurnExecutionAuthority> {
        let key = TurnAuthorityKey {
            tenant: tenant.clone(),
            thread,
            turn,
        };
        let mut catalog = recover_lock(&self.state);
        catalog.interrupted.insert(key.clone());
        let state = catalog.states.get(&key).cloned();
        if let Some(state) = &state {
            recover_lock(state).interruption_requested = true;
        }
        state.map(|state| TurnExecutionAuthority { state })
    }
}

impl TurnExecutionAuthority {
    /// Returns every prepared or running D-7 as fresh handles.
    ///
    /// The handle only mirrors state this authority already cataloged, so it
    /// grants no new allocation or dispatch authority; every guarded
    /// transition still verifies cataloged membership.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn live_attempts(&self) -> Vec<ExecutionAttempt> {
        self.interruption_snapshot().0
    }

    /// Returns one lock-consistent interruption view of live and reserved D-7s.
    ///
    /// The boolean is true when a prepared or running D-7 is hidden by a
    /// terminal reservation. An interrupter must reconcile that Turn instead
    /// of partly closing the visible attempts or reporting it inactive.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn interruption_snapshot(&self) -> (Vec<ExecutionAttempt>, bool) {
        let state = recover_lock(&self.state);
        let terminal_commit_in_flight =
            state
                .attempts
                .values()
                .any(|(_, status, _, terminal_commit_in_flight)| {
                    *terminal_commit_in_flight
                        && matches!(status, ExecutionStatus::Prepared | ExecutionStatus::Running)
                });
        let attempts = state
            .attempts
            .values()
            .filter(|(_, status, _, terminal_commit_in_flight)| {
                !terminal_commit_in_flight
                    && matches!(status, ExecutionStatus::Prepared | ExecutionStatus::Running)
            })
            .map(|(binding, status, started_at_millis, _)| {
                ExecutionAttempt::reconstruct(binding.clone(), *status, *started_at_millis, self)
            })
            .collect();
        (attempts, terminal_commit_in_flight)
    }

    /// Reserves one cataloged D-7 terminal transition before durable commitment.
    ///
    /// A reservation makes the prepared/running state unavailable to a competing
    /// dispatch or cancellation while its conditional canonical write is in
    /// flight. The caller must either mirror the terminal, release the
    /// reservation before any external effect, or retain it for reconciliation
    /// after an external effect has been requested.
    pub(crate) fn reserve_terminal(
        &mut self,
        attempt: &ExecutionAttempt,
    ) -> Result<(), super::ExecutionError> {
        if !Arc::ptr_eq(&self.state, &attempt.authority_state) {
            return Err(super::ExecutionError::TurnMismatch);
        }
        let mut state = recover_lock(&self.state);
        let attempt_id = attempt.binding().attempt_id;
        let Some((binding, status, _, in_flight)) = state.attempts.get_mut(&attempt_id) else {
            return Err(super::ExecutionError::TurnMismatch);
        };
        if binding != attempt.binding() || *status != attempt.status() || *in_flight {
            return Err(super::ExecutionError::AlreadyDispatched);
        }
        *in_flight = true;
        Ok(())
    }

    /// Releases a failed conditional terminal reservation without changing D-7 state.
    pub(crate) fn release_terminal_reservation(&mut self, attempt: &ExecutionAttempt) {
        if !Arc::ptr_eq(&self.state, &attempt.authority_state) {
            return;
        }
        let mut state = recover_lock(&self.state);
        let attempt_id = attempt.binding().attempt_id;
        if let Some((binding, status, _, in_flight)) = state.attempts.get_mut(&attempt_id)
            && binding == attempt.binding()
            && *status == attempt.status()
        {
            *in_flight = false;
        }
    }

    /// Applies one reserved guarded terminal to both authority and local D-7 mirror.
    pub(crate) fn mirror_terminal(
        &mut self,
        attempt: &mut ExecutionAttempt,
        terminal: ExecutionStatus,
    ) -> Result<(), super::ExecutionError> {
        if !Arc::ptr_eq(&self.state, &attempt.authority_state) {
            return Err(super::ExecutionError::TurnMismatch);
        }
        let mut state = recover_lock(&self.state);
        let attempt_id = attempt.binding().attempt_id;
        let Some((binding, status, _, in_flight)) = state.attempts.get_mut(&attempt_id) else {
            return Err(super::ExecutionError::TurnMismatch);
        };
        if binding != attempt.binding() || *status != attempt.status() || !*in_flight {
            return Err(super::ExecutionError::AlreadyDispatched);
        }
        attempt.finish(terminal)?;
        *status = terminal;
        *in_flight = false;
        Ok(())
    }
}

impl ExecutionAttempt {
    /// Rebuilds one cataloged live attempt handle without new lifecycle authority.
    ///
    /// Only the authority catalog reconstructs handles from its own recorded
    /// state; the coordinator still rejects any handle whose identity is not
    /// cataloged, so a reconstructed mirror cannot bypass allocation or
    /// single-dispatch guards.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "T-2 runtime interruption wiring is not complete")
    )]
    pub(crate) fn reconstruct(
        binding: ExactActionBinding,
        status: ExecutionStatus,
        started_at_millis: Option<u64>,
        authority: &TurnExecutionAuthority,
    ) -> Self {
        Self {
            binding,
            status,
            started_at_millis,
            authority_state: Arc::clone(&authority.state),
        }
    }
}

fn key_for(binding: &ExactActionBinding) -> TurnAuthorityKey {
    TurnAuthorityKey {
        tenant: binding.tenant_id.clone(),
        thread: binding.thread_id,
        turn: binding.turn_id,
    }
}
