use crate::helios::{get_client, get_client_latest_finality_head, get_latest_checkpoint};
use log::{error, info};
use std::{env, time::Duration};
use tokio::sync::mpsc;

/// Finality change message
pub struct FinalityChangeMessage {
    pub slot: u64,
}

/// Finality change detector
pub async fn start_helios_finality_change_detector(
    current_head: u64,
) -> (u64, tokio::sync::mpsc::Receiver<FinalityChangeMessage>) {
    // Define helios polling sleep interval
    let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
        .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
        .parse()
        .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

    info!("Fetching helios latest checkpoint.");

    // Get latest beacon checkpoint
    let helios_checkpoint = get_latest_checkpoint().await.unwrap();

    // Get the client from the beacon checkpoint
    let helios_polling_client = get_client(helios_checkpoint).await.unwrap();

    info!("Fetching helios latest finality head.");

    // Get latest slot
    let mut init_latest_beacon_slot = get_client_latest_finality_head(&helios_polling_client)
        .await
        .unwrap();

    // If we get an erronous slot from get_client_latest_finality_head then override it with the current_head
    if current_head > init_latest_beacon_slot {
        init_latest_beacon_slot = current_head;
    }

    // Setup helios finality change mspc
    let (finality_tx, finality_rx) = mpsc::channel(1);

    // Create finality change detector
    tokio::spawn(async move {
        let mut current_slot = init_latest_beacon_slot;
        loop {
            match get_client_latest_finality_head(&helios_polling_client).await {
                Ok(next_slot) => {
                    if next_slot > current_slot {
                        current_slot = next_slot;
                        let _ = finality_tx
                            .send(FinalityChangeMessage { slot: next_slot })
                            .await;
                    }
                }
                Err(e) => {
                    error!("Error checking finality slot head: {}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs_f64(polling_interval_sec)).await;
        }
    });

    (init_latest_beacon_slot, finality_rx)
}
