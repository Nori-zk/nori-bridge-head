# Nori-bridge-head

Relevant workspace is within the `/nori' folder.

## Installation

`cargo build`

## Configuration

Env vars (create a .env file):

```
NORI_HELIOS_POLLING_INTERVAL=1.0 
SP1_PROVER=mock
```

- NORI_HELIOS_POLLING_INTERVAL, dictates the polling interval for new slot heads
- SP1_PROVER, sets the mode for the ZK prover, options are: "mock", "cpu", "cuda" and "network" (note mock executes the program but mocks the zk proof)

Note the network prover requires other env vars to work (FIXME).

## Setup

nori/bin/nori_bridge_head.rs

```
use anyhow::Result;
use nori_bridge_head::{nori_bridge_head::{NoriBridgeHead, NoriBridgeHeadConfig, NoriBridgeHeadMode}, utils::enable_logging_from_cargo_run};

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
```

## Execution

`cargo run --bin nbhead`