// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable-claim doubles for the HTTP transport harness.
//!
//! These implementations live in a sibling file so the parent harness stays
//! below the 1,200-line test exception limit. The winning close counts every
//! canonical terminal — dispatched or prepared-only — so the transport legs
//! keep their exactly-once closing assertions; the durable-claim contract
//! itself is proven by the `PostgreSQL` production-path harness.

use koduck_ai::application::{
    AttemptInsertResolution, AttemptStoreError, DispatchClaimResolution, DurableAttemptTransitions,
    PreparedCloseResolution,
};
use koduck_ai::domain::execution::ExactActionBinding;

use super::{ExistingCommitter, WinningCommitter};

impl DurableAttemptTransitions for WinningCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Ok(AttemptInsertResolution::Inserted)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PreparedCloseResolution, AttemptStoreError> {
        self.calls += 1;
        Ok(PreparedCloseResolution::Won { version: 3 })
    }
}

impl DurableAttemptTransitions for ExistingCommitter {
    fn insert_prepared(
        &mut self,
        _binding: &ExactActionBinding,
        _prepared_at_millis: u64,
    ) -> Result<AttemptInsertResolution, AttemptStoreError> {
        Ok(AttemptInsertResolution::Inserted)
    }

    fn claim_running(
        &mut self,
        _binding: &ExactActionBinding,
        _started_at_millis: u64,
    ) -> Result<DispatchClaimResolution, AttemptStoreError> {
        Ok(DispatchClaimResolution::Claimed { version: 2 })
    }

    fn cancel_prepared_attempt(
        &mut self,
        _binding: &ExactActionBinding,
    ) -> Result<PreparedCloseResolution, AttemptStoreError> {
        Ok(PreparedCloseResolution::Won { version: 3 })
    }
}
