use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use log::{info,debug};
use nori::{
    bridge_head::{
        api::BridgeHead, checkpoint::{load_nb_checkpoint, nb_checkpoint_exists}, observer::{EventObserver, ExampleBridgeHeadEventObserver}
    }, rpcs::consensus::ConsensusHttpProxy, utils::enable_logging_from_cargo_run
};
use std::process;
use tokio::signal::ctrl_c;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable info logging when using cargo --run
    enable_logging_from_cargo_run();

    // Initialise slot head / commitee vars
    let current_slot;
    let store_hash;

    // Start procedure
    if nb_checkpoint_exists() {
        // Warm start procedure
        info!("Loading nori slot checkpoint from file.");
        debug!("Debug printing is enabled.");
        let nb_checkpoint = load_nb_checkpoint().unwrap();
        current_slot = nb_checkpoint.slot;
        store_hash = nb_checkpoint.store_hash;
    } else {
        // Cold start procedure
        // FIXME we should be going from a trusted checkpoint TODO
        info!("Resorting to cold start procedure.");
        (current_slot, store_hash) =
            ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env()
                .get_latest_finality_slot_and_store_hash()
                .await
                .unwrap();
    }

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
    tokio::spawn(bridge_head.run(current_slot, store_hash, None));
    info!("Started nori event loop.");

    // Wait for ctrl-c
    info!("Waiting for ctrl+c exit.");
    ctrl_c()
        .await
        .expect("Failed to listen for shutdown signal");

    process::exit(1);
}
