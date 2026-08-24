// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md
// ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md
// ADR: docs/adr/ADR-0004-provider-stream-completion-normalization.md

//! External protocol and infrastructure adapters around application-owned ports.

pub mod audit;
pub mod execution;
pub mod history;
pub mod http;
pub mod provider;
pub mod tool;

mod unique_json;
