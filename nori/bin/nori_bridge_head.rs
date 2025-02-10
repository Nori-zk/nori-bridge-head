use anyhow::Result;
use nori::{bridge_head::{NoriBridgeHead, NoriBridgeHeadConfig, NoriBridgeHeadMode}, utils::enable_logging_from_cargo_run };

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();
    // Create nori-bridge-head configuration
    let config = NoriBridgeHeadConfig::new(NoriBridgeHeadMode::Finality);
    // Initialize nori-bridge-head (nbh)
    let mut nbh: NoriBridgeHead = NoriBridgeHead::new(config).await;
    // Start nbh
    nbh.run().await;
    Ok(())
}