use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::{ConsensusSpec, MainnetConsensusSpec};
use helios_ethereum::rpc::{http_rpc::HttpRpc, ConsensusRpc};
//use crate::rpcs::consensus::{get_client, get_client_latest_finality_slot, get_client_latest_finality_update_and_slot, get_latest_checkpoint, FinalityUpdateAndSlot};
use crate::rpcs::consensus::ConsensusHttpProxy;
use log::{debug, error, info, warn};
use nori_sp1_helios_primitives::types::{DualProofInputsWithWindow, ProofInputsWithWindow};
use std::{process, time::Duration};
use tokio::{sync::mpsc, time::interval};

// So this needs to be aware of the input slot and store hash...
// We run this validate_and_prepare_proof_inputs(input_slot: u64, store_hash: FixedBytes<32>)
// We need a receiver for this and get this from advance most of the time or from internal state via get_latest_finality_slot_and_store_hash on cold start
// so we could be give a seed input_slot and store_hash and otherwise just update when we need to
// We continuously query until we have some change... which we will have validated if we dont get any response from validate_and_prepare_proof_inputs
// When we do have change we emit the input output slots along with the proof_inputs to the bridge head to be forwarded to any observer.

/// Represents an update for the finality change detector. Be it a staging or bridge advance
///
/// Contains the beacon slot number and the associated consensus store hash from which we want prove a transition proof from.
#[derive(Clone)]
pub struct FinalityChangeDetectorUpdate {
    /// The beacon slot number which indicates where we are proving from during the transition proof
    pub slot: u64,
    /// The consensus store hash corresponding to the slot.
    pub store_hash: FixedBytes<32>,
} // Or these represent a staging event.

/// Represents an input job for the finality change detector.
///
/// Contains the beacon slot number and the associated consensus store hash from which we want prove a transition proof from.
pub struct FinalityChangeDetectorJobInput {
    /// The beacon slot number which indicates where we are proving from during the transition proof
    pub slot: u64,
    /// The consensus store hash corresponding to the slot.
    pub store_hash: FixedBytes<32>,
    // Here we should add two more the next_slot and the next_store_hash which are the destinations of a proof which is inflight
    // but note we need two types as it become somewhat disjointed between the actor and handler methods
    // Perhaps options are the right idea.

    // These represent the expected output slot / store hash of the the proof which starts from 'slot'
    pub next_expected_output: Option<FinalityChangeDetectorUpdate>,
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
/// On success, the actor sends back a tuple `validated_consensus_mpt_proof_input_with_window` which
/// contains the fully validated transition inputs suitable for zk proof generation and the block and
/// slot window for which it pertains.
///
/// On failure, it sends an error via the results channel indicating invalid transitions
/// or issues with provider data, instead of returning directly.
///
/// # Returns
/// A tuple containing:
/// - `mpsc::Sender<FinalityChangeDetectorInput>`: channel to submit validation jobs.
/// - `mpsc::Receiver<Result<ProofInputsWithWindows<S>, anyhow::Error>>`: channel to receive validation results.
pub fn validate_and_prepare_proof_inputs_actor<S, R>() -> (
    mpsc::Sender<FinalityChangeDetectorJobInput>,
    mpsc::Receiver<Result<DualProofInputsWithWindow<S>, anyhow::Error>>,
)
where
    S: ConsensusSpec + Send + 'static,
    R: ConsensusRpc<S> + std::fmt::Debug + Send + 'static,
{
    let (job_tx, mut job_rx) = mpsc::channel::<FinalityChangeDetectorJobInput>(1);
    let (result_tx, result_rx) =
        mpsc::channel::<Result<DualProofInputsWithWindow<S>, anyhow::Error>>(1);

    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            let res = if let Some(next_expected_output) = job.next_expected_output {
                let consensus_http_proxy = ConsensusHttpProxy::<S, R>::try_from_env();

                // We might get the same input slot for both current and next we shouldnt compute both...

                debug!(
                    "Calculating proof input for CURRENT window, input slot: {}",
                    job.slot
                );
                debug!(
                    "Calculating proof input for NEXT window, input slot: {}",
                    next_expected_output.slot
                );

                // Prepare job inputs from job.slot -> new slot (unknown one)
                let current_res = consensus_http_proxy.prepare_consensus_mpt_proof_inputs(
                    job.slot,
                    job.store_hash,
                    true,
                );
                //.await;

                // Prepare job inputs from next expected output slot of a staged job -> new slot (unknown one)
                let next_res = consensus_http_proxy.prepare_consensus_mpt_proof_inputs(
                    next_expected_output.slot,
                    next_expected_output.store_hash,
                    true,
                );
                //.await;

                let (current, next) = tokio::join!(current_res, next_res);

                let current_res = match current {
                    Ok(val) => {
                        debug!("Dual window proof input calculation. Success in CURRENT window proof input validation. Input slot: {}, Output slot: {}", val.input_slot, val.expected_output_slot);
                        val
                    }
                    Err(e) => {
                        debug!("Dual window proof input calculation. Error in CURRENT window proof input validation:\n{}", e);
                        let _ = result_tx.send(Err(e)).await;
                        continue;
                    }
                };

                // Here is it fails we will just exclude it? turn this into an ok if it happens to often.
                let next_res = match next {
                    Ok(val) => {
                        debug!("Dual window proof input calculation. Success in NEXT window proof input validation. Input slot: {}, Output slot: {}", val.input_slot, val.expected_output_slot);
                        val
                    }
                    Err(e) => {
                        debug!("Dual window proof input calculation. Error in NEXT window proof input validation:\n{}", e);
                        let _ = result_tx.send(Err(e)).await;
                        continue;
                    }
                };

                DualProofInputsWithWindow::<S> {
                    current_window: current_res,
                    next_window: Some(next_res),
                }
            } else {
                let current_res = ConsensusHttpProxy::<S, R>::try_from_env()
                    .prepare_consensus_mpt_proof_inputs(job.slot, job.store_hash, true)
                    .await;

                let current_res = match current_res {
                    Ok(val) => {
                        debug!("Solo current window proof input calculation. Success in CURRENT window proof input validation. Input slot: '{}', Output slot '{}'", val.input_slot, val.expected_output_slot);
                        val
                    }
                    Err(e) => {
                        debug!("Solo current window proof input calculation. Error in CURRENT window proof input validation:\n{}", e);
                        let _ = result_tx.send(Err(e)).await;
                        continue;
                    }
                };

                DualProofInputsWithWindow::<S> {
                    current_window: current_res,
                    next_window: None,
                }
            };

            // Here we are preparing A -> D (While A->B is in process [if it is in the pipeline])
            // But we should also be preparing B->D

            if result_tx.send(Ok(res)).await.is_err() {
                break;
            }
        }
    });

    (job_tx, result_rx)
}

async fn try_start_validation_job(
    validation_job_tx: &mpsc::Sender<FinalityChangeDetectorJobInput>,
    slot: u64,
    store_hash: FixedBytes<32>,
    next_expected_output: &Option<FinalityChangeDetectorUpdate>,
    in_flight: &mut bool,
) -> Result<(), ()> {
    let filtered_next = match next_expected_output {
        Some(n) if n.slot != slot => Some(n.clone()),
        Some(n) => {
            info!(
                "Suppressing next_expected_output because its input slot '{}' equals the current input slot '{}'",
                n.slot, slot
            );
            None
        }
        None => None,
    };

    let job = FinalityChangeDetectorJobInput {
        slot,
        store_hash,
        next_expected_output: filtered_next,
    };

    match validation_job_tx.send(job).await {
        Ok(_) => {
            *in_flight = true;
            Ok(())
        }
        Err(e) => {
            error!("Validation actor job channel closed unexpectedly: {:?}", e);
            Err(())
        }
    }
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
/// - `mpsc::Receiver<ProofInputsWithWindow>`: Channel for receiving validated transition proof inputs and window information
/// - `mpsc::Sender<ProofInputsWithWindow>`: Channel for updating monitoring the current trusted slot / consensus store hash
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
    // need an pipeline_inflight_output_slot to represent the end of the window slot of a proof that is currently being processed
    // by the pipeline if it exists such that we can compute windowed proof inputs from this as an input slot in case that the inflight
    // job succeeds
    pipeline_inflight_next_expected_output: Option<FinalityChangeDetectorUpdate>,
) -> (
    u64,
    mpsc::Receiver<DualProofInputsWithWindow<S>>,
    mpsc::Sender<FinalityChangeDetectorUpdate>,
    mpsc::Sender<FinalityChangeDetectorUpdate>,
)
where
    S: ConsensusSpec + Send + 'static,
    R: ConsensusRpc<S> + std::fmt::Debug + Send + 'static,
{
    dotenv::dotenv().ok();

    let polling_interval_sec: f64 = std::env::var("NORI_HELIOS_POLLING_INTERVAL")
        .unwrap_or_else(|_| "10.0".to_string())
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
    let (finality_advance_input_tx, mut finality_advance_input_rx) =
        mpsc::channel::<FinalityChangeDetectorUpdate>(1);
    let (finality_stage_input_tx, mut finality_stage_input_rx) =
        mpsc::channel::<FinalityChangeDetectorUpdate>(1);

    // Channels for validation actor (job requests and results)
    let (validation_job_tx, mut validation_result_rx) =
        validate_and_prepare_proof_inputs_actor::<S, R>();

    tokio::spawn(async move {
        // State tracking
        info!("Consensus change detector has started.");
        // Update the cache which is used to filter what input slot to process from (avoids accepting when the rpc giving us earlier values).
        let mut latest_slot = slot;

        let mut in_flight = false; // indicates if validation request is outstanding
        let mut stale: bool = false; // indicates if we had some update while our last dual proof inputs were being calculated

        let mut tick_interval = interval(Duration::from_secs_f64(polling_interval_sec));

        let mut next_expected_output: Option<FinalityChangeDetectorUpdate> =
            pipeline_inflight_next_expected_output;

        loop {
            tokio::select! {
                // Receive input updates when the bridge head stages a job.
                Some(update) = finality_stage_input_rx.recv() => {
                    // Do something with this stage event
                    // these represent the output slot of the staged job
                    // they represent the other half of the dual input that we need to calculate proofs from
                    //next_expected_output_slot = Some(update.slot); // expected_output_slot
                    //next_expected_output_store_hash = Some(update.store_hash); // expected_output_store_hash
                    let next_expected_output_slot = update.slot;
                    next_expected_output = Some(update);
                    stale = true;
                    info!("Finality transition detector notified of new staging event. Next window expected proof input slot: '{}'", next_expected_output_slot);
                },
                // Receive input updates when the bridge head advances.
                Some(update) = finality_advance_input_rx.recv() => {
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
                    // Not sure if the below is needed
                    stale = true;

                    info!("Finality transition detector notified of bridge advance. Current input slot: '{}'", slot);
                },

                // Receive validation results
                Some(result) = validation_result_rx.recv() => {
                    in_flight = false;

                    match result {
                        Ok(dual_validated_proof_inputs) => {
                            if stale {
                                debug!("Received dual_validated_proof_inputs but result was stale dropping.");
                                // Drop this result
                                stale = false;
                                // We should immediately start an new proof validation job validation_job_tx.send(job). FIXME
                                // Note this is common logic to what we have below.... perhaps a helper function?
                                if try_start_validation_job(&validation_job_tx, slot, store_hash, &next_expected_output, &mut in_flight).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                            let input_slot = dual_validated_proof_inputs.current_window.input_slot;
                            let output_slot = dual_validated_proof_inputs.current_window.expected_output_slot;
                            // Only emit output if the input_slot matches current and our output is ahead of the current slot
                            // We will accept this new output slot in the window beginning at 'slot'
                            if dual_validated_proof_inputs.current_window.input_slot == slot {
                                if dual_validated_proof_inputs.current_window.expected_output_slot > latest_slot {

                                    // Do we have a next window?
                                    if let Some(ref next_window) = dual_validated_proof_inputs.next_window {
                                        // Is the next window's output slot greater than the current slot
                                        if next_window.expected_output_slot > latest_slot {
                                            if finality_output_tx.send(dual_validated_proof_inputs).await.is_err() {
                                                // Receiver dropped, exit task, should maybe process exit?
                                                break;
                                            }
                                        }  else {
                                            debug!("No change detected for next window's result: '{}' when the latest_slot was: '{}'. Ignoring.", output_slot, latest_slot);
                                        }
                                    }
                                    else if finality_output_tx.send(dual_validated_proof_inputs).await.is_err() {
                                        // Receiver dropped, exit task, should maybe process exit?
                                        break;
                                    }
                                    latest_slot = output_slot;
                                }
                                else {
                                    debug!(
                                        "No change detected for current window's result was output_slot: '{}' when the latest_slot was: '{}'. Ignoring.",
                                        output_slot,
                                        latest_slot
                                    );
                                }

                            } else {
                                debug!(
                                    "Stale validation result received for current window. result input slot: '{}', current slot: '{}'. Ignoring.",
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
                    if !in_flight && try_start_validation_job(&validation_job_tx, slot, store_hash, &next_expected_output, &mut in_flight).await.is_err() {
                        break;
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
        finality_advance_input_tx,
        finality_stage_input_tx,
    )
}
