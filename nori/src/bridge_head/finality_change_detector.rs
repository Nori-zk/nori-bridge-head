use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::{ConsensusSpec, MainnetConsensusSpec};
use helios_ethereum::rpc::{http_rpc::HttpRpc, ConsensusRpc};
//use crate::rpcs::consensus::{get_client, get_client_latest_finality_slot, get_client_latest_finality_update_and_slot, get_latest_checkpoint, FinalityUpdateAndSlot};
use log::{debug, error, info};
use nori_sp1_helios_primitives::types::ProofInputs;
use std::{process, time::Duration};
use tokio::{sync::mpsc, time::interval};

use crate::rpcs::consensus::ConsensusHttpProxy;

// So this needs to be aware of the input slot and store hash...
// We run this validate_and_prepare_proof_inputs(input_slot: u64, store_hash: FixedBytes<32>)
// We need a receiver for this and get this from advance most of the time or from internal state via get_latest_finality_slot_and_store_hash on cold start
// so we could be give a seed input_slot and store_hash and otherwise just update when we need to
// We continuously query until we have some change... which we will have validated if we dont get any response from validate_and_prepare_proof_inputs
// When we do have change we emit the input output slots along with the proof_inputs to the bridge head to be forwarded to any observer.

/// Represents an input job for the finality change detector.
///
/// Contains the beacon slot number and the associated consensus store hash from which we want prove a transition proof from.
pub struct FinalityChangeDetectorInput {
    /// The beacon slot number which indicates where we are proving from during the transition proof
    pub slot: u64,
    /// The consensus store hash corresponding to the slot.
    pub store_hash: FixedBytes<32>,
}

/// Spawns an asynchronous actor responsible for querying RPC providers and locally validating
/// consensus state transitions via `validate_and_prepare_proof_inputs`.
///
/// This actor receives validation jobs specifying an `input_slot` and `store_hash`.
/// For each job, it calls the consensus RPC proxy to:
/// - fetch raw updates,
/// - simulate the zkVM consensus logic that will be applied but does so natively to avoid the expense,
/// - and validate the state transition from `input_slot` to `output_slot`.
///
/// On success, the actor sends back a tuple `(input_slot, output_slot, validated_proof_inputs)`
/// representing the fully validated transition inputs suitable for zk proof generation.
///
/// On failure, it sends an error via the results channel indicating invalid transitions
/// or issues with provider data, instead of returning directly.
///
/// # Returns
/// A tuple containing:
/// - `mpsc::Sender<FinalityChangeDetectorInput>`: channel to submit validation jobs.
/// - `mpsc::Receiver<Result<(u64, u64, ProofInputs<S>), anyhow::Error>>`: channel to receive validation results.
pub struct FinalityChangeDetectorOutput<S: ConsensusSpec> {
    /// The beacon slot number from which the transition proof was generated.
    pub input_slot: u64,
    /// The beacon slot number that the transition proof targets.
    pub output_slot: u64,
    /// The validated proof inputs for the slot transition.
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
                .prepare_consensus_mpt_proof_inputs(job.slot, job.store_hash, true)
                .await;

            if result_tx.send(res).await.is_err() {
                break;
            }
        }
    });

    (job_tx, result_rx)
}

/// Spawns a continuous finality change detector that orchestrates multi-RPC validated state transitions from the current bridge head slot.
///
/// This actor system maintains consensus proof readiness by:
/// 1. Starting with an initial slot/store_hash (cold start or injected state)
/// 2. Continuously polling for new finality slots at regular intervals
/// 3. Validating generated proof inputs derived from RPC data in parallel
/// 4. Only emitting transitions where ≥1 trusted RPC provides valid proof inputs within the timeout period
///
/// Employs a multi-layered validation strategy:
/// - Queries multiple consensus RPC endpoints simultaneously
/// - Performs native execution of zkVM logic for transition validation
/// - Automatically filters out providers that return fraudulent/invalid data/or that take to long
///
/// # Arguments
/// * `slot` - Initial trusted slot to monitor from
/// * `store_hash` - Corresponding consensus store state hash for integrity checking
///
/// # Returns
/// Tuple containing:
/// - `u64`: Initial latest finalized slot from consensus layer for we which have validated proof input
/// - `mpsc::Receiver<FinalityChangeDetectorOutput>`: Channel for receiving validated transition proof inputs
/// - `mpsc::Sender<FinalityChangeDetectorInput>`: Channel for updating monitoring the current trusted slot / consensus store hash
///
/// # Behavior Details
/// - Maintains internal state of current head (slot + store hash)
/// - Accepts updates via input channel (from bridge head advancement)
/// - Periodically triggers validation checks based on NORI_HELIOS_POLLING_INTERVAL
/// - Only considers results from the current accepted head
/// - Automatically retries failed validations on next polling interval
/// - Terminates process on unrecoverable channel failures
///
/// # Validation Guarantees
/// Each emitted transition has been:
/// 1. Validated against ≥1 trusted RPC provider
/// 2. Had its state transition logic verified through native execution
/// 3. Passed cryptographic consistency checks (store_hash chain)
/// 4. Been confirmed to be a transition from the current head to prevent stale emissions
pub async fn start_validated_consensus_finality_change_detector<S, R>(
    mut slot: u64,
    mut store_hash: FixedBytes<32>,
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

    tokio::spawn(async move {
        // State tracking
        info!("Consensus change detector has started.");
        let mut latest_slot = slot;
        let mut in_flight = false; // indicates if validation request is outstanding

        let mut tick_interval = interval(Duration::from_secs_f64(polling_interval_sec));

        loop {
            tokio::select! {
                // Receive input updates
                Some(update) = finality_input_rx.recv() => {
                    slot = update.slot;
                    store_hash = update.store_hash;
                    // we should probably just override the slot here FIXME
                    // our last computed proof input was from a different input slot and thus is not really valid
                    // when the observer calls advance -> api advance gets called this is with the output slot of that proof
                    // which is our new input slot. We need to check finality changes from this point!
                    // so we should reset our latest_slot because we need a new proof input from this slot to finality.
                    // so we need to mark the latest_slot as our slot. This may mean we emit multiple finality change
                    // detections for the same beacon finality... but currently we need to do that as otherwise we would be
                    // re proving from an input slot we have already emitted a proof from.
                    //if latest_slot < slot {
                    latest_slot = slot;
                    //}
                },

                // Receive validation results
                Some(result) = validation_result_rx.recv() => {
                    in_flight = false;

                    match result {
                        Ok((input_slot, output_slot, validated_proof_inputs)) => {
                            // Only emit output if the input_slot matches current and our output is ahead of the current slot
                            if input_slot == slot {
                                if output_slot > latest_slot {
                                    if finality_output_tx.send(FinalityChangeDetectorOutput {
                                        input_slot,
                                        output_slot,
                                        validated_proof_inputs,
                                    }).await.is_err() {
                                        // Receiver dropped, exit task
                                        break;
                                    }
                                    latest_slot = output_slot;
                                }
                                else {
                                    debug!(
                                        "No change detected result was output_slot: '{}' when the latest_slot was: '{}'. Ignoring.",
                                        output_slot,
                                        latest_slot
                                    );
                                }

                            } else {
                                debug!(
                                    "Stale validation result received. result input slot: '{}', current slot: '{}'. Ignoring.",
                                    input_slot,
                                    slot
                                );
                            }
                        }
                        Err(e) => {
                            debug!("Validation error in change detector ignoring: {:?}", e);
                        }
                    }
                },

                // Tick event - try to start validation if none in-flight
                _ = tick_interval.tick() => {
                    if !in_flight {

                        let job = FinalityChangeDetectorInput {
                            slot,
                            store_hash,
                        };
                        if validation_job_tx.send(job).await.is_ok() {
                            in_flight = true;
                        } else {
                            error!("Validation actor job channel closed unexpectedly.");
                            break; // stop the loop if sending jobs is impossible
                        }
                    }
                }
            }
        }
        // Finality change detector broke
        error!("Finality change detector broke.");
        process::exit(1);
    });

    (
        init_latest_beacon_slot,
        finality_output_rx,
        finality_input_tx,
    )
}
