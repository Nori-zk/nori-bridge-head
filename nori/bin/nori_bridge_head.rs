use anyhow::Result;
use log::info;
use nori::{
    bridge_head::{
        api::BridgeHead,
        observer::{EventObserver, ExampleBridgeHeadEventObserver},
    },
    utils::enable_logging_from_cargo_run,
};
use std::process;
use tokio::signal::ctrl_c;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    // Create bridge head and fetch event reciever
    info!("Initing bridge head");
    let (bridge_head_cmd_handle, bridge_head) = BridgeHead::new().await;
    let bridge_head_event_receiver = bridge_head.event_receiver();

    // Start the bridge head receiver
    info!("Starting nori event observer.");
    tokio::spawn(async move {
        let mut bridge_head_observer = ExampleBridgeHeadEventObserver::new(bridge_head_cmd_handle);
        bridge_head_observer.run(bridge_head_event_receiver).await;
    });
    info!("Started nori event observer.");

    // Start the bridge head
    info!("Starting nori event loop, with observer.");
    tokio::spawn(bridge_head.run());
    info!("Started nori event loop.");

    // Wait for ctrl-c
    info!("Waiting for ctrl+c exit.");
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
