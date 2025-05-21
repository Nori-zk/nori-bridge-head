use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::{ConsensusSpec, MainnetConsensusSpec};
use helios_ethereum::rpc::{http_rpc::HttpRpc, ConsensusRpc};
//use crate::rpcs::consensus::{get_client, get_client_latest_finality_slot, get_client_latest_finality_update_and_slot, get_latest_checkpoint, FinalityUpdateAndSlot};
use log::{error, info};
use nori_sp1_helios_primitives::types::ProofInputs;
use std::{env, time::Duration};
use tokio::{sync::mpsc, time::interval};

use crate::rpcs::consensus::{Client, ConsensusHttpProxy};

// So this needs to be aware of the input slot and store hash...
// We run this validate_and_prepare_proof_inputs(input_slot: u64, store_hash: FixedBytes<32>)
// We need a receiver for this and get this from advance most of the time or from internal state via get_latest_finality_slot_and_store_hash on cold start
// so we could be give a seed input_slot and store_hash and otherwise just update when we need to
// We continuously query until we have some change... which we will have validated if we dont get any response from validate_and_prepare_proof_inputs
// When we do have change we emit the input output slots along with the proof_inputs to the bridge head to be forwarded to any observer.

pub struct FinalityChangeDetectorInput {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

pub struct FinalityChangeDetectorOutput<S: ConsensusSpec> {
    pub input_slot: u64,
    pub output_slot: u64,
    pub validated_proof_inputs: ProofInputs<S>,
}

/// Finality change detector
pub async fn start_helios_finality_change_detector<S: ConsensusSpec, R: ConsensusRpc<S> + std::fmt::Debug>(
    slot: u64,
    store_hash: FixedBytes<32>,
) -> (
    u64,
    tokio::sync::mpsc::Receiver<FinalityChangeDetectorOutput<S>>,
    tokio::sync::mpsc::Sender<FinalityChangeDetectorInput>,
) {
    dotenv::dotenv().ok();

    // Define helios polling sleep interval
    let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
        .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
        .parse()
        .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

    info!("Fetching helios latest checkpoint.");
    let init_latest_beacon_slot =
        ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env()
            .get_latest_finality_slot()
            .await
            .unwrap();

    // Setup consensus finality change mspc
    let (finality_output_tx, finality_output_rx) = mpsc::channel(1);
    let (finality_input_tx, mut finality_input_rx): (
        tokio::sync::mpsc::Sender<FinalityChangeDetectorInput>,
        tokio::sync::mpsc::Receiver<FinalityChangeDetectorInput>,
    ) = mpsc::channel(1);

    // Spawn detector task
    tokio::spawn(async move {
        let mut tick_interval = interval(Duration::from_secs_f64(polling_interval_sec));
        // input_slot and store_hash are captured mutable, no clone needed for primitives or fixed bytes
        let mut input_slot = slot;
        let mut current_store_hash = store_hash;

        loop {
            tokio::select! {
                Some(update) = finality_input_rx.recv() => {
                    input_slot = update.slot;
                    current_store_hash = update.store_hash;
                },
                _ = tick_interval.tick() => {
                    match ConsensusHttpProxy::<S, R>::try_from_env()
                        .validate_and_prepare_proof_inputs(input_slot, current_store_hash)
                        .await
                    {
                        Ok((given_input_slot, output_slot, validated_proof_inputs)) => {
                            if finality_output_tx.send(FinalityChangeDetectorOutput {
                                input_slot: given_input_slot,
                                output_slot,
                                validated_proof_inputs,
                            }).await.is_err() {
                                // Receiver dropped, exit loop
                                break;
                            }
                        }
                        Err(_) => {
                            // Silently ignore errors
                        }
                    }
                }
            }
        }
    });

    (
        init_latest_beacon_slot,
        finality_output_rx,
        finality_input_tx,
    )
}

/*// Get latest beacon checkpoint
let helios_checkpoint = get_latest_checkpoint().await.unwrap();

// Get the client from the beacon checkpoint
let helios_polling_client = get_client(helios_checkpoint).await.unwrap();

info!("Fetching helios latest finality head.");

// Get latest slot


let mut init_latest_beacon_slot  = get_client_latest_finality_slot(&helios_polling_client)
    .await
    .unwrap();*/

// If we get an erronous slot from get_client_latest_finality_head then override it with the current_head
/*if current_slot > init_latest_beacon_slot {
    init_latest_beacon_slot = current_slot;
}*/

// Create rx tx pair for receiving slot and store hash from parent caller.
//let ()
