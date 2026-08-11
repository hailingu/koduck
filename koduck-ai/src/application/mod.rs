// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

//! Provider-neutral application orchestration and consumer-owned ports.

mod durability;
mod ports;
mod runner;

pub use durability::{AppendPolicy, BufferLimitError, UnpublishedBuffer};
pub use ports::*;
pub use runner::TurnRunner;
