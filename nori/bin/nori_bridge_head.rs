use anyhow::Result;
use log::info;
use nori::{
    bridge_head::BridgeHead,
    event_handler:: ExampleEventHandler,
    utils::enable_logging_from_cargo_run,
};
use std::process;
use std::sync::Arc;
use tokio::{signal::ctrl_c, sync::Mutex};


#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    info!("Starting");

    let bridge_head = Arc::new(Mutex::new(BridgeHead::new().await));

    info!("Inited bridge head");

    // Create the ProofListener
    let proof_listener = ExampleEventHandler::new(bridge_head.clone());

    info!("Inited proof listener");

    // Add the ProofListener as a boxed listener
    let mut bridge_head_guard = bridge_head.lock().await;

    info!("Locked bridge head");

    info!("Starting nori event loop.");

    // Start the event loop
    bridge_head_guard.run(proof_listener).await;

    info!("Adding proof listener");

       // Drop the guard
    drop(bridge_head_guard);

    info!("Waiting for exit.");

    // Wait for ctrl-c
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
