use anyhow::Result;
use async_trait::async_trait;
use log::info;
use nori::{
    bridge_head_actor::BridgeHeadActor, bridge_head_event_loop::NoriBridgeHeadProofMessage,
    event_dispatcher::EventListener, utils::enable_logging_from_cargo_run,
};
use std::sync::Arc;
use tokio::{signal::ctrl_c, sync::Mutex};

pub struct ProofListener {
    bridge_head: Arc<Mutex<BridgeHeadActor>>, // Use Arc<Mutex<NoriBridgeHead>> for shared ownership
}

impl ProofListener {
    pub fn new(bridge_head: Arc<Mutex<BridgeHeadActor>>) -> Self {
        Self { bridge_head }
    }
}

#[async_trait]
impl EventListener<NoriBridgeHeadProofMessage> for ProofListener {
    async fn on_event(&mut self, data: NoriBridgeHeadProofMessage) -> Result<()> {
        println!("Got proof message: {}", data.slot);

        // Acquire lock and call advance()
        let mut bridge = self.bridge_head.lock().await; // Correctly await the lock
        bridge.advance().await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    info!("Starting");

    let bridge_head = Arc::new(Mutex::new(BridgeHeadActor::new().await));

    info!("Inited bridge head");

    // Create the ProofListener
    let proof_listener = ProofListener::new(bridge_head.clone());

    info!("Inited proof listener");

    // Add the ProofListener as a boxed listener
    let mut bridge_head_guard = bridge_head.lock().await;

    info!("Locked bridge head");

    info!("Starting nori event loop.");

    // Start the event loop
    bridge_head_guard.run().await;

    info!("Adding proof listener");

    // Add proof listener
    bridge_head_guard.add_proof_listener(proof_listener).await?;

    // Drop the guard
    drop(bridge_head_guard);

    info!("Waiting for exit.");

    // Wait for ctrl-c
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    info!("Shutdown signal received");

    // Get lock again to send shutdown command
    let mut bridge_head_guard = bridge_head.lock().await;
    bridge_head_guard.shutdown().await?;
    drop(bridge_head_guard);

    info!("Shutdown command issued. Exiting... Hopefully...");

    Ok(())
}
