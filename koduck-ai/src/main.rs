// ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md

use koduck_ai::runtime::{RuntimeConfig, RuntimeError, run};

#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    run(RuntimeConfig::from_process_environment()?).await
}
