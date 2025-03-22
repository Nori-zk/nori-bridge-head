use anyhow::Result;
use log::info;
use nori::{
    beacon_finality_change_detector::{api::FinalityChangeDetector, observer::BeaconFinalityChangeEmitter},
    bridge_head::{api::BridgeHead, observer::{EventObserver, ExampleEventObserver}},
    utils::enable_logging_from_cargo_run
};
use std::process;
use tokio::signal::ctrl_c;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    info!("Starting");

    info!("Inited bridge head");

    let (current_head, bridge_head_cmd_handle, bridge_head_beacon_change_handle, bridge_head) = BridgeHead::new().await;

    info!("Starting nori event observer.");
    let bridge_head_event_receiver = bridge_head.event_receiver();
    tokio::spawn(async move {
        let mut bridge_head_observer = ExampleEventObserver::new(bridge_head_cmd_handle);
        bridge_head_observer.run(bridge_head_event_receiver).await;
    });    
    info!("Started nori event observer.");

    info!("Starting nori event loop, with observer.");
    tokio::spawn(bridge_head.run());
    info!("Started nori event loop.");

    info!("Initing beacon finality change detector and emitting observer");

    let beacon_finality_change_detector = FinalityChangeDetector::new(current_head).await;
    let beacon_change_emitter = BeaconFinalityChangeEmitter::new(bridge_head_beacon_change_handle);

    info!("Starting beacon finality change detector, with emitting observer");
    tokio::spawn(beacon_finality_change_detector.run(Box::new(beacon_change_emitter)));
    info!("Started beacon finality change detector");

    info!("Waiting for exit.");

    // Wait for ctrl-c
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
