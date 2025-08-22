use crate::rpcs::consensus::ConsensusHttpProxy;
use alloy_primitives::FixedBytes;
use helios_consensus_core::consensus_spec::{ConsensusSpec, MainnetConsensusSpec};
use helios_ethereum::rpc::{http_rpc::HttpRpc, ConsensusRpc};
use log::{debug, error, info};
use nori_sp1_helios_primitives::types::DualProofInputsWithWindow;
use std::{process, time::Duration};
use tokio::{sync::mpsc, time::interval};

// So this needs to be aware of the input slot and store hash...
// We run this validate_and_prepare_proof_inputs(input_slot: u64, store_hash: FixedBytes<32>)
// We need a receiver for this and get this from advance most of the time or from internal state via get_latest_finality_slot_and_store_hash on cold start
// so we could be give a seed input_slot and store_hash and otherwise just update when we need to
// We continuously query until we have some change... which we will have validated if we dont get any response from validate_and_prepare_proof_inputs
// When we do have change we emit the input output slots along with the proof_inputs to the bridge head to be forwarded to any observer.

/// Represents an update to the consensus finality change detector.
///
/// This enum is used to notify the `FinalityChangeDetector` actor of either:
/// - a new **next window** (derived from the expected output slot of an in-flight staged proof), or
/// - an updated **current window** (when a staged proof has completed processing, been finalized,
///   and the bridge head advances).
///
/// The detector uses these updates to maintain internal state, trigger validation, and
/// produce dual proof inputs over two distinct windows:
/// - Current window: `(input slot → latest finalized slot)`
/// - Next window: `(expected output slot of the in-flight staged proof → latest finalized slot)`
///
/// # Update variants
///
/// - `BridgeAdvancement`  
///   Defines the start of the **current window** from the bridge head’s slot and store hash
///   after a staged proof has finalized. This advances the active head, updates the start of the
///   current window, and triggers recalculation of proof inputs.
///
/// - `ProofStaging`  
///   Defines the start of the **next window** from the expected output slot and store hash of
///   an in-flight staged proof. This anchors the next window and is used to prepare upcoming
///   proof inputs in parallel with the current window.
///
/// # Fields
/// - `slot`  
///   The beacon slot that defines the start of a window:
///   - `ProofStaging`: expected output slot of the in-flight staged proof (start of the **next window**).
///   - `BridgeAdvancement`: bridge head slot (start of the **current window**).
/// - `store_hash`  
///   The consensus store hash at `slot`:
///   - `ProofStaging`: hash at the staged proof’s expected output slot.
///   - `BridgeAdvancement`: hash at the bridge head slot.
#[derive(Clone)]
pub struct FinalityChangeDetectorUpdate {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
}

/// FinalityChangeDetector job input struct describing the window starts the detector worker must
/// materialize into proof inputs.
///
/// ## Current Window
/// The **current window** is defined by `BridgeAdvancement` update events. It starts at the bridge
/// head’s slot with the corresponding store hash after a staged proof has finalized. The worker
/// uses this information to generate proof inputs for the **current window**.
///
/// ## Next Window
/// The **next window** is defined by `ProofStaging` update events. It starts at the expected output
/// slot of an in-flight staged proof with its store hash. The worker uses this information to prepare
/// proof inputs for the **next window** in parallel with the **current window**.
///
/// ## Modes
/// - **Solo**  
///   Compute proof inputs only for the **current window**.
/// - **Dual**  
///   Compute proof inputs for both the **current window** and the **next window** concurrently.
///
/// ## Fields
/// - `slot` – start slot of the **current window**.
/// - `store_hash` – store hash at the **current window** slot.
/// - `next_expected_output` – optional `FinalityChangeDetectorUpdate` containing the start slot
///   and store hash for the **next window**, used in dual mode.
pub struct FinalityChangeDetectorJobInput {
    pub slot: u64,
    pub store_hash: FixedBytes<32>,
    pub next_expected_output: Option<FinalityChangeDetectorUpdate>,
}

/// Spawns an asynchronous actor responsible for generating and validating consensus proof inputs
/// for the `FinalityChangeDetector` actor, based on incoming `FinalityChangeDetectorJobInput` jobs.
///
/// Each job specifies the information needed to generate and validate proof inputs for one or two windows:
/// - **Current Window**: represents the active finalized state of the bridge. It starts at the bridge head’s
///   slot with the corresponding store hash after a staged proof has accepted and finalized by the bridge.
///   The worker uses this slot and store hash to generate proof inputs for the current window.
/// - **Next Window (Optional)**: represents an in-progress staged proof that has not yet finalized by the bridge.
///   It starts at the expected output slot of the currently staged proof, along with its store hash. When provided,
///   the worker uses this information to prepare proof inputs for the next window concurrently with the current window.
///
/// The actor performs the following for each window:
/// 1. Queries the consensus RPC (through `ConsensusHttpProxy`) to fetch relevant updates.
/// 2. Simulates zkVM consensus logic natively to prepare proof inputs, without performing full zk proof computation.
/// 3. Validates the state transition from the window’s start slot to the expected output slot for that window.
///
/// Upon success, it sends back a `DualProofInputsWithWindow<S>` struct containing fully validated proof inputs:
/// - `current_window`: proof inputs for the current window.
/// - `next_window`: optional proof inputs for the next window.
///
/// Upon failure (invalid transition or RPC issues) for either window, it sends an error via the results channel.
///
/// # Modes
/// - **Solo**: only the current window proof inputs are computed (when `next_expected_output` is `None`).
/// - **Dual**: both current and next window proof inputs are computed concurrently (when `next_expected_output` is `Some`).
///
/// # Returns
/// A tuple containing:
/// - `mpsc::Sender<FinalityChangeDetectorJobInput>`: channel to submit validation jobs.
/// - `mpsc::Receiver<Result<DualProofInputsWithWindow<S>, anyhow::Error>>`: channel to receive validation results.
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

                debug!(
                    "Calculating proof input for CURRENT window, input slot: {}",
                    job.slot
                );
                debug!(
                    "Calculating proof input for NEXT window, input slot: {}",
                    next_expected_output.slot
                );

                // Prepare job inputs from current bridge header slot job.slot -> new slot (unknown one)
                let current_res = consensus_http_proxy.prepare_consensus_mpt_proof_inputs(
                    job.slot,
                    job.store_hash,
                    true,
                );

                // Prepare job inputs from next expected output slot of a staged job -> new slot (unknown one)
                let next_res = consensus_http_proxy.prepare_consensus_mpt_proof_inputs(
                    next_expected_output.slot,
                    next_expected_output.store_hash,
                    true,
                );

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

            if result_tx.send(Ok(res)).await.is_err() {
                break;
            }
        }
    });

    (job_tx, result_rx)
}

/// Computes proof inputs for the current window and optionally for the next window,
/// then attempts to start a validation job with those inputs.
///
/// This function prepares a `FinalityChangeDetectorJobInput` and sends it to the validation actor. The actor:
///
/// - Computes **proof inputs for the current window** based on the given slot and store hash.
/// - Optionally computes **proof inputs for the next window** if `next_expected_output` is provided.
/// - Ensures that duplicate jobs are not submitted when the next window matches the current one.
/// - Tracks in-flight validation jobs, marking them as outstanding until processed.
///
/// # Arguments
/// * `validation_job_tx` - Channel sender to submit validation jobs to the validation actor.
/// * `slot` - The input slot corresponding to the start of the current window.
/// * `store_hash` - The hash of the store at the input slot for the current window.
/// * `next_expected_output` - Optional slot and store hash for computing proof inputs for the next window,
///   following a currently staged proof's window.
/// * `in_flight` - Mutable reference indicating whether a validation job is currently outstanding in the validation actor.
///
/// # Returns
/// * `Ok(())` if the job was successfully submitted and marked in-flight.
/// * `Err(())` if the job channel is closed and the actor cannot process further jobs.
async fn try_start_validation_job(
    validation_job_tx: &mpsc::Sender<FinalityChangeDetectorJobInput>,
    slot: u64,
    store_hash: FixedBytes<32>,
    next_expected_output: &Option<FinalityChangeDetectorUpdate>,
    in_flight: &mut bool,
) -> Result<(), ()> {
    // This may be unnecessary as start_validated_consensus_finality_change_detector sets next_expected_output
    // to none if next_expected_output_some.slot == the bridge header slot
    // leaving the print in for testing
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
            // FIXME should probably process exit
            Err(())
        }
    }
}

/// Spawns an asynchronous finality change detector that continuously monitors the latest finality slot,
/// bridge head advancement, and staged proofs, while orchestrating multi-RPC validated state transitions
/// to generate validated consensus proof inputs.
///
/// This detector manages consensus proof readiness by:
/// 1. Starting from an initial slot and store hash (cold start or injected state)
/// 2. Tracking the latest beacon finality slot via RPC
/// 3. Listening for **bridge head advancement events** to update the input slot of the **current window**
///    (the slot from which the bridge head's header slot currently tracks)
/// 4. Listening for **staged proof events** to register the expected output of the proof currently in-flight,
///    which becomes the input slot of the **next window**.
/// 5. If the current in-flight proof succeeds the **next window's** proof inputs must be taken. On failure,
///    the bridge must resort to the latest **current window** proof inputs. In either case both windows target the latest accepted consensus finality slot.
/// 6. Validating proof inputs across windows using multiple RPC sources in parallel
/// 7. Emitting proof inputs only when a valid and trusted result is available
///    and the expected output slot for each window is ahead of the last emitted slot (the latest accepted consensus finality slot), avoiding the cases
///    where the RPC can give consensus finality slot's with a lower value than previous RPC queries.
/// 8. Automatically handling stale or in-flight validation results and restarting validation jobs as needed
///
/// Arguments:
/// - `slot`: The initial input slot for the **current window**, from which finality change detection begins (anchored to the bridge's slot header).
/// - `store_hash`: The hash of the store at the input slot for the **current window**.
/// - `pipeline_inflight_next_expected_output`: Optional in-flight proof job representing the
///   expected output slot and store hash of a staged proof currently being processed, i.e. the **next window's** input slot.  
///
/// Purpose:
///   The detector computes the **current window’s** proof inputs from the bridge head’s finality slot
///   to the latest consensus finality slot, while simultaneously pre-computing the **next window’s**
///   proof inputs. The **next window** assumes the in-flight staged job will succeed, starting from its
///   output slot and extending to the latest consensus finality slot.  
///   This allows the bridge to advance immediately on success using the **next window’s** inputs,
///   or fall back to the **current window’s** inputs on failure.  
///   Both windows are validated in parallel to guarantee readiness in either case.
///
/// Returns a tuple containing:
/// 1. `u64`: The latest beacon finality slot at the time of detector startup
/// 2. `mpsc::Receiver<DualProofInputsWithWindow<S>>`: Channel receiving validated proof inputs
///    for the **current window** and, when applicable, the **next window**
/// 3. `mpsc::Sender<FinalityChangeDetectorUpdate>`: Channel for sending bridge head advancement
///    notifications, updating the input slot of the **current window**
/// 4. `mpsc::Sender<FinalityChangeDetectorUpdate>`: Channel for sending staged proof notifications,
///    registering the expected **next window** input slot
///
/// Behavior:
/// - Polls RPC endpoints at intervals defined by `NORI_HELIOS_POLLING_INTERVAL` (default 10 seconds)
/// - Handles simultaneous events:
///     - Bridge head advancement → updates **current window** input slot
///     - Staged proof update → registers **next window** input slot (assuming the staged proof will succeed)
///     - Validation result → accepted only if valid, non-stale, and strictly ahead of the last accepted consensus finality slot
///     - Polling tick → triggers validation if no job is in-flight
/// - Enforces that proof input computation is always **contiguous**:
///     `[input_slot, store_hash] → [expected_output_slot, store_hash]`,
///   where the output slot of one window becomes the input slot of the next
/// - Filters out invalid, delayed, or duplicate results
/// - Maintains `latest_slot` to prevent re-emitting proofs with output slots less than
///   the latest accepted consensus finality slot
/// - Runs indefinitely until the process exits or a critical error occurs
///
/// Type Parameters:
/// - `S`: The consensus specification type implementing `ConsensusSpec`
/// - `R`: The RPC interface type implementing `ConsensusRpc<S>` and `Debug`
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
        // Update the latest_slot cache which is used to filter what consensus finality slot we will be targeting,
        // it is used to avoids acceptance of a transition output slot, when the rpc gives us earlier slot values,
        // which happens annoyingly frequently around a transition.
        let mut latest_slot = slot;
        // Indicates if validation request to the actor is outstanding
        let mut in_flight = false;
        // Indicates if we had some update, while our last dual proof inputs were being calculated by the actor.
        let mut stale: bool = false;
        // Interval between RPC requests (bundles of request per prepare_consensus_mpt_proof_inputs invocation)
        let mut tick_interval = interval(Duration::from_secs_f64(polling_interval_sec));
        // Option for if we have a currently staged job in the pipeline, it represents the window start which is the
        // output slot of the currently staged job, it can be used to calculate input for if the staged job succeeds.
        let mut next_expected_output: Option<FinalityChangeDetectorUpdate> =
            pipeline_inflight_next_expected_output;

        loop {
            tokio::select! {
                // Receive input updates when the bridge head stages a job.
                Some(update) = finality_stage_input_rx.recv() => {
                    // The staged events represent the output slot / hash of a staged job,
                    // they represent the other half of the dual input that we need to calculate proofs from.

                    // Cache the update
                    let next_expected_output_slot = update.slot;
                    next_expected_output = Some(update);

                    // Mark any inflight jobs as stale
                    if in_flight {
                        stale = true;
                    }

                    info!("Finality transition detector notified of new staging event. Next window expected proof input slot: '{}'", next_expected_output_slot);
                },
                // Receive input updates when the bridge head advances.
                Some(update) = finality_advance_input_rx.recv() => {
                    // Cache the update
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
                    if in_flight {
                        stale = true;
                    }

                    // Remove the next_expected_output if it represents the same window start as our current window start
                    if let Some(ref next_expected_output_some) = next_expected_output {
                        if next_expected_output_some.slot == slot {
                            info!("Scrubbing next_expected_output as its the same as out slot");
                            next_expected_output = None
                        }
                    }

                    info!("Finality transition detector notified of bridge advance. Current input slot: '{}'", slot);
                },

                // Receive validation results
                Some(result) = validation_result_rx.recv() => {
                    // We received output from our actor thus we have nothing in-flight anymore.
                    in_flight = false;

                    match result {
                        Ok(dual_validated_proof_inputs) => {
                            // If our job received here was stale drop it and try again.
                            if stale {
                                debug!("Received dual_validated_proof_inputs but result was stale dropping.");
                                // Drop this result
                                stale = false;
                                // Immediately start an new proof validation job validation_job_tx.send(job)
                                if try_start_validation_job(&validation_job_tx, slot, store_hash, &next_expected_output, &mut in_flight).await.is_err() {
                                    break;
                                }
                                continue;
                            }

                            // Update our input and output slot from the current windows proof inputs result.
                            let input_slot = dual_validated_proof_inputs.current_window.input_slot;
                            let output_slot = dual_validated_proof_inputs.current_window.expected_output_slot;

                            // Only emit output if the input_slot matches the current windows input 'slot' and our output is ahead 
                            // of the latest_slot (the latest accepted consensus finality slot), thus representing progress.
                            // We will accept this new output slot in the window beginning at 'slot'
                            if dual_validated_proof_inputs.current_window.input_slot == slot {
                                if dual_validated_proof_inputs.current_window.expected_output_slot > latest_slot {

                                    debug!("We could have SENT an update SOLO");

                                    // Do we have a next window?
                                    if let Some(ref next_window) = dual_validated_proof_inputs.next_window {
                                        // Is the next window's output slot greater than the current slot
                                        if next_window.expected_output_slot > latest_slot {
                                            if finality_output_tx.send(dual_validated_proof_inputs).await.is_err() {
                                                // Receiver dropped, exit task, should maybe process exit?
                                                break;
                                            }
                                            debug!("We SENT an update DUAL");
                                        }  else {
                                            debug!("No change detected for next window's result: '{}' when the latest_slot was: '{}'. Ignoring.", next_window.expected_output_slot, latest_slot);
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
