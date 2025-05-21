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

pub fn validate_and_prepare_proof_inputs_actor<S, R>() -> (
    mpsc::Sender<FinalityChangeDetectorInput>,
    mpsc::Receiver<Result<(u64, u64, ProofInputs<S>), anyhow::Error>>,
)
where
    S: ConsensusSpec + Send + 'static,
    R: ConsensusRpc<S> + std::fmt::Debug + Send + 'static,
{
    let (job_tx, mut job_rx) = mpsc::channel::<FinalityChangeDetectorInput>(1);
    let (result_tx, result_rx) =
        mpsc::channel::<Result<(u64, u64, ProofInputs<S>), anyhow::Error>>(1);

    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            let res = ConsensusHttpProxy::<S, R>::try_from_env()
                .validate_and_prepare_proof_inputs(job.slot, job.store_hash)
                .await;

            if result_tx.send(res).await.is_err() {
                break;
            }
        }
    });

    (job_tx, result_rx)
}
pub async fn start_helios_finality_change_detector<S, R>(
    slot: u64,
    store_hash: FixedBytes<32>,
) -> (
    u64,
    mpsc::Receiver<FinalityChangeDetectorOutput<S>>,
    mpsc::Sender<FinalityChangeDetectorInput>,
)
where
    S: ConsensusSpec + Send + 'static,
    R: ConsensusRpc<S> + std::fmt::Debug + Send + 'static,
{
    dotenv::dotenv().ok();

    let polling_interval_sec: f64 = std::env::var("NORI_HELIOS_POLLING_INTERVAL")
        .unwrap_or_else(|_| "1.0".to_string())
        .parse()
        .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

    info!("Fetching helios latest checkpoint.");
    let init_latest_beacon_slot =
        ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env()
            .get_latest_finality_slot()
            .await
            .unwrap();

    // Channels for finality detector output and input updates
    let (finality_output_tx, finality_output_rx) = mpsc::channel(1);
    let (finality_input_tx, mut finality_input_rx) =
        mpsc::channel::<FinalityChangeDetectorInput>(1);

    // Channels for validation actor (job requests and results)
    let (validation_job_tx, mut validation_result_rx) =
        validate_and_prepare_proof_inputs_actor::<S, R>();

    // State tracking
    let mut current_slot = slot;
    let mut current_store_hash = store_hash;
    let mut in_flight = false; // indicates if validation request is outstanding

    tokio::spawn(async move {
        let mut tick_interval =
            tokio::time::interval(Duration::from_secs_f64(polling_interval_sec));

        loop {
            tokio::select! {
                // Receive input updates
                Some(update) = finality_input_rx.recv() => {
                    current_slot = update.slot;
                    current_store_hash = update.store_hash;
                },

                // Receive validation results
                Some(result) = validation_result_rx.recv() => {
                    in_flight = false;

                    match result {
                        Ok((input_slot, output_slot, validated_proof_inputs)) => {
                            // Only emit output if the input_slot matches current
                            if input_slot == current_slot {
                                if finality_output_tx.send(FinalityChangeDetectorOutput {
                                    input_slot,
                                    output_slot,
                                    validated_proof_inputs,
                                }).await.is_err() {
                                    // Receiver dropped, exit task
                                    break;
                                }
                            } else {
                                log::warn!(
                                    "Stale validation result received. result input_slot: {}, current slot: {}. Ignoring.",
                                    input_slot,
                                    current_slot
                                );
                            }
                        }
                        Err(e) => {
                            log::warn!("Validation error in change detector ignoring: {:?}", e);
                        }
                    }
                },

                // Tick event - try to start validation if none in-flight
                _ = tick_interval.tick() => {
                    if !in_flight {
                        let job = FinalityChangeDetectorInput {
                            slot: current_slot,
                            store_hash: current_store_hash,
                        };
                        if validation_job_tx.send(job).await.is_ok() {
                            in_flight = true;
                        } else {
                            log::error!("Validation actor job channel closed unexpectedly.");
                            break; // stop the loop if sending jobs is impossible
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
