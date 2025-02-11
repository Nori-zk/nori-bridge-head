use std::sync::Arc;

use anyhow::{Ok, Result};
use async_trait::async_trait;
use log::info;
use nori::{
    bridge_head::{
        NoriBridgeHead, NoriBridgeHeadConfig, NoriBridgeHeadMode, NoriBridgeHeadProofMessage,
    },
    event_dispatcher::EventListener,
    utils::enable_logging_from_cargo_run,
};
use tokio::sync::Mutex;

pub struct ProofListener {
    bridge_head: Arc<Mutex<NoriBridgeHead>>, // Use Arc<Mutex<NoriBridgeHead>> for shared ownership
}

impl ProofListener {
    pub fn new(bridge_head: Arc<Mutex<NoriBridgeHead>>) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl EventListener<NoriBridgeHeadProofMessage> for ProofListener {
    async fn on_event(&mut self, data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("Got proof message: {}", data.slot);

        // Acquire lock and call advance()
        let mut bridge = self.bridge_head.lock().await; // Correctly await the lock
        bridge.advance().await;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    // Create NoriBridgeHead configuration
    let config = NoriBridgeHeadConfig::new(NoriBridgeHeadMode::Finality);
    let bridge_head = Arc::new(Mutex::new(NoriBridgeHead::new(config).await));

    // Create the ProofListener
    let proof_listener = ProofListener::new(bridge_head.clone());

    // Add the ProofListener as a boxed listener
    bridge_head.lock().await.add_proof_listener(Box::new(proof_listener));

    // Start the bridge head
    bridge_head.lock().await.run().await;

    Ok(())
}

// 9b6fcc43-5166-4349-8b5c-49f96993b882