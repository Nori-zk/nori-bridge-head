use helios_consensus_core::verify_finality_update;
use nori::{
    bridge_head::finality_change_detector::start_validated_consensus_finality_change_detector,
    rpcs::consensus::{get_checkpoint, get_client, get_client_latest_finality_update},
};

#[tokio::main]
async fn main() {
    let (slot, mut rx) = start_validated_consensus_finality_change_detector(0).await;

    loop {
        if let Some(event) = rx.recv().await {
            /*println!("Validating finality update");
            let helios_checkpoint = get_checkpoint(event.slot).await.unwrap();

            // Re init helios client and extract references
            let mut helios_update_client: helios_ethereum::consensus::Inner<helios_consensus_core::consensus_spec::MainnetConsensusSpec, helios_ethereum::rpc::http_rpc::HttpRpc> = get_client(helios_checkpoint).await.unwrap();

            let expected_slot = helios_update_client.expected_current_slot();
            let finality_update = get_client_latest_finality_update(&helios_update_client).await.unwrap();

            let config = helios_update_client.config;
            verify_finality_update(
                &finality_update,
                expected_slot,
                &helios_update_client.store,
                config.chain.genesis_root,
                &config.forks,
            ).unwrap();

            println!("Update valid");*/
        }
    }
}
