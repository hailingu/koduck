// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Runtime-owned lookup and strong retention for Turn execution authority.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use super::{
    AttemptBudget, ExactActionBinding, TurnAuthorityState, TurnExecutionAuthority, recover_lock,
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
    states: Mutex<HashMap<TurnAuthorityKey, Arc<Mutex<TurnAuthorityState>>>>,
}

impl TurnAuthorityCatalog {
    pub(crate) fn authority_for(&self, binding: &ExactActionBinding) -> TurnExecutionAuthority {
        let key = key_for(binding);
        let mut states = recover_lock(&self.states);
        if let Some(state) = states.get(&key) {
            return TurnExecutionAuthority {
                state: Arc::clone(state),
            };
        }
        let state = Arc::new(Mutex::new(TurnAuthorityState {
            key: key.clone(),
            profile_id: binding.profile_id.clone(),
            profile_version: binding.profile_version.clone(),
            budget: AttemptBudget::new(),
            attempts: BTreeMap::new(),
        }));
        states.insert(key, Arc::clone(&state));
        TurnExecutionAuthority { state }
    }
}

fn key_for(binding: &ExactActionBinding) -> TurnAuthorityKey {
    TurnAuthorityKey {
        tenant: binding.tenant_id.clone(),
        thread: binding.thread_id,
        turn: binding.turn_id,
    }
}
