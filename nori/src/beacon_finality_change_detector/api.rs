use super::event_observer::BeaconFinalityChangeEventObserver;
use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::FinalityUpdate};
use helios_ethereum::{
    consensus::Inner,
    rpc::{http_rpc::HttpRpc, ConsensusRpc},
};
use log::{error, info};
use sp1_helios_script::{get_client, get_latest_checkpoint};
use std::{env, time::Duration};

pub struct FinalityChangeDetectorEventLoop {
    current_slot: u64,
    helios_polling_client: Inner<MainnetConsensusSpec, HttpRpc>,
    polling_interval_sec: f64,
    observer: Option<Box<dyn BeaconFinalityChangeEventObserver + Send + Sync>>,
}

impl FinalityChangeDetectorEventLoop {
    pub async fn new() -> Self {
        dotenv::dotenv().ok();

        // Define sleep interval
        let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
            .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
            .parse()
            .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

        // Get latest beacon checkpoint
        let helios_checkpoint = get_latest_checkpoint().await;

        // Get the client from the beacon checkpoint
        let helios_polling_client = get_client(helios_checkpoint).await;

        let current_slot = helios_polling_client
            .store
            .finalized_header
            .clone()
            .beacon()
            .slot;

        Self {
            current_slot,
            helios_polling_client,
            polling_interval_sec,
            observer: None,
        }
    }

    async fn check_finality_next_head(&self) -> Result<u64> {
        // Get finality slot head
        info!("Polling helios finality slot head.");
        let finality_update: FinalityUpdate<MainnetConsensusSpec> = self
            .helios_polling_client
            .rpc
            .get_finality_update()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

        // Extract latest slot
        Ok(finality_update.finalized_header.beacon().slot)
    }

    pub async fn run_loop(
        mut self,
        observer: Box<dyn BeaconFinalityChangeEventObserver + Send + Sync>,
    ) {
        self.observer = Some(observer);

        /* Initial observation
        if let Some(observer) = &mut self.observer {
            observer.as_mut().on_beacon_change(self.current_slot).await;
        }
        */

        loop {
            match self.check_finality_next_head().await {
                Ok(next_head) => {
                    if next_head > self.current_slot {
                        self.current_slot = next_head;
                        if let Some(observer) = &mut self.observer {
                            observer.as_mut().on_beacon_change(next_head).await;
                        }
                    }
                }
                Err(e) => {
                    error!("Error checking finality slot head: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs_f64(self.polling_interval_sec)).await;
        }
    }
}
