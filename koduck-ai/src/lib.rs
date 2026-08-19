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
#[path = "../tests/internal/test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "../tests/internal/test_migrations.rs"]
pub(crate) mod test_migrations;

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
#[path = "../tests/internal/cand_2_http.rs"]
mod cand_2_http_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_limits.rs"]
mod cand_2_limits_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_postgres.rs"]
mod cand_2_postgres_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_retry.rs"]
mod cand_2_retry_tests;
#[cfg(test)]
#[path = "../tests/internal/cand_2_runtime_assembly.rs"]
mod cand_2_runtime_assembly_tests;
