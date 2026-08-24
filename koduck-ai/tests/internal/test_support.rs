// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Shared doubles for the crate-internal CAND-2 harnesses.

/// Implements the coordinator-side durable-transition port for one
/// process-local test double: preparation always records and every locally
/// claimed dispatch wins the durable slot, mirroring the process-local-only
/// arbitration the logic-level legs exercise (ADR-0003 TC-12).
///
/// Doubles that must answer from real durable state — like the wrapped
/// `SqlxExecutionAttemptStore` liveness fixture — keep a hand-written
/// delegating implementation instead of this macro.
macro_rules! process_local_durable_claims {
    ($double:ty) => {
        impl $crate::application::DurableAttemptTransitions for $double {
            fn insert_prepared(
                &mut self,
                _binding: &$crate::domain::execution::ExactActionBinding,
                _prepared_at_millis: u64,
            ) -> Result<
                $crate::application::AttemptInsertResolution,
                $crate::application::AttemptStoreError,
            > {
                Ok($crate::application::AttemptInsertResolution::Inserted)
            }

            fn claim_running(
                &mut self,
                _binding: &$crate::domain::execution::ExactActionBinding,
                _started_at_millis: u64,
            ) -> Result<
                $crate::application::DispatchClaimResolution,
                $crate::application::AttemptStoreError,
            > {
                Ok($crate::application::DispatchClaimResolution::Claimed { version: 2 })
            }

            fn cancel_prepared_attempt(
                &mut self,
                _binding: &$crate::domain::execution::ExactActionBinding,
            ) -> Result<
                $crate::application::PreparedCloseResolution,
                $crate::application::AttemptStoreError,
            > {
                Ok($crate::application::PreparedCloseResolution::Won { version: 3 })
            }
        }
    };
}

pub(crate) use process_local_durable_claims;
