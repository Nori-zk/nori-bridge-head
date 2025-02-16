use anyhow::Result;
use log::info;
use nori::{bridge_head::{api::BridgeHead, event_observer::ExampleEventObserver}, utils::enable_logging_from_cargo_run};
use std::process;
use tokio::signal::ctrl_c;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    info!("Starting");

    let (bridge_head_handle, bridge_head) = BridgeHead::new().await;

    info!("Inited bridge head");

    let bridge_head_observer = ExampleEventObserver::new(bridge_head_handle);

    info!("Inited bridge head observer");

    info!("Starting nori event loop, with observer.");

    // Start the event loop
    tokio::spawn(bridge_head.run(Box::new(bridge_head_observer)));

    info!("Waiting for exit.");

    // Wait for ctrl-c
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
