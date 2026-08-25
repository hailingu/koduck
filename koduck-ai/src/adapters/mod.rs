// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md

//! External protocol and infrastructure adapters around application-owned ports.

pub mod audit;
pub mod execution;
pub mod history;
pub mod http;
pub mod provider;
mod strict_json;
pub mod tool;
