use crate::helios::{get_client, get_client_latest_finality_head, get_latest_checkpoint};
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::{
    consensus::Inner,
    rpc::http_rpc::HttpRpc,
};
use log::{error, info};
use std::{env, time::Duration};
use super::observer::BeaconFinalityChangeEventObserver;

pub struct FinalityChangeDetector {
    current_slot: u64,
    helios_polling_client: Inner<MainnetConsensusSpec, HttpRpc>,
    polling_interval_sec: f64,
    observer: Option<Box<dyn BeaconFinalityChangeEventObserver + Send + Sync>>,
}

pub struct FinalityChangeMessage {
    pub slot: u64,
}

impl FinalityChangeDetector {
    pub async fn new(current_slot: u64) -> Self {
        dotenv::dotenv().ok();

        // Define sleep interval
        let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
            .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
            .parse()
            .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

        // Get latest beacon checkpoint
        let helios_checkpoint = get_latest_checkpoint().await.unwrap();

        // Get the client from the beacon checkpoint
        let helios_polling_client = get_client(helios_checkpoint).await.unwrap();

        Self {
            current_slot,
            helios_polling_client,
            polling_interval_sec,
            observer: None,
        }
    }

    pub async fn run(
        mut self,
        observer: Box<dyn BeaconFinalityChangeEventObserver + Send + Sync>,
    ) {
        info!("Finality change detector is starting.");
        self.observer = Some(observer);

        loop {
            match get_client_latest_finality_head(&self.helios_polling_client).await {
                Ok(next_head) => {
                    if next_head > self.current_slot {
                        self.current_slot = next_head;
                        if let Some(observer) = &mut self.observer {
                            let _ = observer.as_mut().on_beacon_change(next_head).await;
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
