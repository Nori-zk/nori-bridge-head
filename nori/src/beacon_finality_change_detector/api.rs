use async_trait::async_trait;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::{consensus::Inner, rpc::http_rpc::HttpRpc};
use sp1_helios_script::{get_client, get_latest_checkpoint};

use super::event_loop::FinalityChangeDetectorEventLoop;

#[async_trait]
pub trait BeaconFinalityChangeEventListener {
    async fn on_beacon_change(&mut self, slot: u64) {}
    // run method goes here...
}

pub struct FinalityChangeDetector {}

impl FinalityChangeDetector {
    pub async fn run(event_listener: Box<dyn BeaconFinalityChangeEventListener + Send + Sync>) {
        let event_loop = FinalityChangeDetectorEventLoop::new(event_listener).await;
        tokio::spawn(event_loop.run_loop());
    }
}