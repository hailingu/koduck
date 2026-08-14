// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md

//! Provider-neutral turn orchestration owned by the Koduck AI service.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod runtime;

#[cfg(test)]
extern crate self as koduck_ai;

#[cfg(test)]
#[path = "../tests/internal/cand_2_approval.rs"]
mod cand_2_approval_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_cancellation.rs"]
mod cand_2_cancellation_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_denials.rs"]
mod cand_2_denial_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_execution.rs"]
mod cand_2_execution_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_limits.rs"]
mod cand_2_limits_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_retry.rs"]
mod cand_2_retry_tests;
