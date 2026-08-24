// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Durable-claim doubles for the shared cancellation harness.
//!
//! These implementations live in a sibling file so the parent harness stays
//! below the 1,200-line test exception limit. The scripted and counting
//! committers keep hand-written durable transitions so every prepared-only
//! close consumes the same scripted sequence — or increments the same call
//! counter — as a dispatched terminal commit, preserving each leg's
//! exactly-once and failure-sequencing assertions (ADR-0003 TC-10/TC-12).
//! The durable-claim contract itself is proven by the `PostgreSQL`
//! production-path harness.

use koduck_ai::application::{
    AttemptCommitError, AttemptCommitResult, AttemptInsertResolution, AttemptStoreError,
    DispatchClaimResolution, DurableAttemptTransitions, PreparedCloseResolution,
};
use koduck_ai::domain::execution::ExactActionBinding;

use super::{SequencedCommitter, UnavailableCommitter, WinningCommitter};

/// Maps one scripted commit outcome onto its prepared-only close meaning:
/// a winning commit closes, a conflict means another owner progressed the
/// row, and a fenced or unavailable commit keeps its failure class.
fn scripted_close(
    result: &Result<AttemptCommitResult, AttemptCommitError>,
) -> Result<PreparedCloseResolution, AttemptStoreError> {
    match result {
        Ok(_) => Ok(PreparedCloseResolution::Won { version: 3 }),
        Err(AttemptCommitError::Conflict) => Ok(PreparedCloseResolution::Progressed {
            status: koduck_ai::domain::execution::ExecutionStatus::Running,
            version: 2,
        }),
        Err(AttemptCommitError::Fenced) => Ok(PreparedCloseResolution::Fenced),
        Err(AttemptCommitError::Unavailable) => Err(AttemptStoreError::Unavailable),
    }
}

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

impl DurableAttemptTransitions for SequencedCommitter {
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
        let result = self
            .results
            .pop_front()
            .expect("sequenced committer has a close for every call");
        scripted_close(&result)
    }
}

impl DurableAttemptTransitions for UnavailableCommitter {
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
        Err(AttemptStoreError::Unavailable)
    }
}
