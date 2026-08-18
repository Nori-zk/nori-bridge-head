use super::finality_change_detector::start_validated_consensus_finality_change_detector;
use crate::rpcs::consensus::ConsensusHttpProxy;
use super::handles::{Command, CommandHandle};
use super::notice_messages::{
    TransitionNoticeBridgeHeadMessage, TransitionNoticeBridgeHeadMessageExtension,
    TransitionNoticeExtensionBridgeHeadAdvanced,
    TransitionNoticeExtensionBridgeHeadFinalityTransitionDetected,
    TransitionNoticeExtensionBridgeHeadJobCreated, TransitionNoticeExtensionBridgeHeadJobFailed,
    TransitionNoticeExtensionBridgeHeadJobSucceeded, TransitionNoticeExtensionBridgeHeadStarted,
};
use super::validate::validate_env;
use crate::bridge_head::finality_change_detector::FinalityChangeDetectorUpdate;
use crate::sp1_prover::{ProverJobOutput, finality_update_job};
use crate::sp1_prover_config::ProverConfig;
use alloy_primitives::FixedBytes;
use anyhow::{Error, Result};
use chrono::{SecondsFormat, Utc};
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use log::{error, info};
use nori_sp1_helios_primitives::types::{
    DualProofInputsWithWindow, ProofInputsWithWindow, ProofOutputs, VerifiedRequest,
};
use serde::{Deserialize, Serialize};
use sp1_sdk::SP1ProofWithPublicValues;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Proof types

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProofMessage {
    pub input_slot: u64,
    pub input_block_number: u64,
    pub input_store_hash: FixedBytes<32>,
    pub output_slot: u64,
    pub output_block_number: u64,
    pub output_store_hash: FixedBytes<32>,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
    pub verified_contract_storage_slots_root: FixedBytes<32>,
    pub next_sync_committee_hash: FixedBytes<32>,
    pub proof_request_queue_address: alloy_primitives::Address,
    pub verified_requests: Vec<VerifiedRequest>,
    /// Queue cursor this proof resumed from.
    pub input_queue_cursor: u64,
    /// Queue cursor after this proof settles.
    pub output_queue_cursor: u64,
    pub elapsed_sec: f64,
}

struct ProverJob {
    inputs_with_window: ProofInputsWithWindow<MainnetConsensusSpec>,
    start_instant: Instant,
}

pub struct ProverJobError {
    pub job_id: u64,
    pub error: Error,
}

// Implement Display trait for user-friendly error messages
impl fmt::Display for ProverJobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Prover job {} failed: {}", self.job_id, self.error)
    }
}

// Implement Debug for ProverJobError
impl fmt::Debug for ProverJobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProverJobError {{ job_id: {}, source: {:?} }}",
            self.job_id, self.error
        )
    }
}

// Implement std::error::Error for ProverJobError
impl StdError for ProverJobError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.error)
    }
}

// Event enum
#[derive(Clone)]
pub enum BridgeHeadEvent {
    ProofMessage(ProofMessage),
    NoticeMessage(TransitionNoticeBridgeHeadMessage),
}

/// Core bridge head implementation that manages proof generation and state transitions
///
/// Handles:
/// - Proof generation workflow
/// - State management
/// - Event observation
pub struct BridgeHead {
    /// Current finalized slot head
    current_slot: u64,
    /// Target slot to advance to
    next_slot: u64,
    /// Unique identifier for prover jobs
    job_id: u64,
    /// Active prover jobs mapped by job ID
    prover_jobs: HashMap<u64, ProverJob>,
    /// Channel for receiving bridge head commands
    command_rx: Option<mpsc::Receiver<Command>>,
    /// Channel for informing consensus finality change detector above bridge head stage event
    finality_stage_input_tx: Option<mpsc::Sender<FinalityChangeDetectorUpdate>>,
    /// Channel for receiving job results
    job_rx: Option<mpsc::Receiver<Result<ProverJobOutput, ProverJobError>>>,
    /// Channel for sending job results
    job_tx: mpsc::Sender<Result<ProverJobOutput, ProverJobError>>,
    /// Channel for emitting proof and notice events
    event_tx: mpsc::Sender<BridgeHeadEvent>,
    /// FixedBytes representing the store hash
    store_hash: FixedBytes<32>,
}

impl BridgeHead {
    pub async fn new() -> (CommandHandle, mpsc::Receiver<BridgeHeadEvent>, Self) {
        validate_env(&[
            "NORI_SOURCE_EXECUTION_HTTP_RPCS",
            "NORI_SOURCE_CONSENSUS_HTTP_RPCS",
            "NORI_TOKEN_BRIDGE_ADDRESS",
            "NORI_SOURCE_CHAIN_ID"
        ]);

        // Initialise slot head to dummy values (will be set to real values after run is invoked)
        let current_slot = 0u64;
        // let init_latest_beacon_slot = 0u64;
        let store_hash = FixedBytes::<32>::ZERO;

        // Setup command mpsc
        let (command_tx, command_rx) = mpsc::channel(2);

        // Create command handle
        let input_command_handle = CommandHandle::new(command_tx);

        // Create bounded sp1 job mpsc with capacity 2, currently 1 job in serial pipeline,
        // but sized for future optimisation where we could compute next sp1 job while current is being processed (by proof conversion / eth processor)
        let (job_tx, job_rx) = mpsc::channel(2);

        // Create bounded mpsc channel for emitting events to observer/strategy with backpressure
        let (event_tx, event_rx) = mpsc::channel(16);

        (
            input_command_handle,
            event_rx,
            BridgeHead {
                current_slot,
                next_slot: 0,
                job_id: 0,
                prover_jobs: HashMap::new(),
                command_rx: Some(command_rx),
                finality_stage_input_tx: None,
                job_rx: Some(job_rx),
                job_tx,
                event_tx,
                store_hash,
            },
        )
    }

    // ================================================================================================
    // Event dispatchers
    // ================================================================================================

    ///  Emit proofs
    async fn trigger_listener_with_proof(
        &mut self,
        payload: ProofMessage,
    ) -> Result<(), mpsc::error::SendError<BridgeHeadEvent>> {
        self.event_tx.send(BridgeHeadEvent::ProofMessage(payload)).await
    }

    /// Emit notices
    async fn trigger_listener_with_notice(
        &mut self,
        extension: TransitionNoticeBridgeHeadMessageExtension,
    ) -> Result<(), mpsc::error::SendError<BridgeHeadEvent>> {
        let now = Utc::now();
        let iso_string = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let notice_message = extension.into_message(iso_string);
        self.event_tx.send(BridgeHeadEvent::NoticeMessage(notice_message)).await
    }

    // ================================================================================================
    // SP1 Job + Handlers
    // ================================================================================================

    /// Create SP1 prover job
    async fn stage_transition_proof(
        &mut self,
        sp1_config: Arc<ProverConfig>,
        proof_inputs_with_window: ProofInputsWithWindow<MainnetConsensusSpec>,
    ) -> Result<()> {
        // Get job id
        self.job_id += 1;
        let job_id: u64 = self.job_id;

        // Print received job message
        info!(
            "Nori bridge head updater received a new job {}. Spawning a new worker.",
            job_id
        );

        // Insert job details into map
        self.prover_jobs.insert(
            job_id,
            ProverJob {
                inputs_with_window: proof_inputs_with_window.clone(),
                start_instant: Instant::now(),
            },
        );

        // Create job data tx
        let tx = self.job_tx.clone();

        // Clone job arguments
        let current_slot = self.current_slot;
        let store_hash = self.store_hash;
        let expected_output_slot = proof_inputs_with_window.expected_output_slot;
        let expected_output_store_hash = proof_inputs_with_window.expected_output_store_hash;
        // This job drains its whole batch, so the cursor it will settle at is
        // the one it started from plus the entries it covers.
        let expected_output_queue_cursor = proof_inputs_with_window
            .proof_inputs
            .queue_storage
            .input_cursor
            + proof_inputs_with_window
                .proof_inputs
                .queue_storage
                .entries
                .len() as u64;
        let inputs = proof_inputs_with_window.proof_inputs;

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            // Execute job
            let proof_result = finality_update_job(sp1_config, job_id, current_slot, inputs).await;

            // Send appropriate tx Ok or Err
            // Bounded channel requires .await. If send fails, receiver dropped (system shutting down).
            match proof_result {
                Ok(prover_job_output) => {
                    if tx.send(Ok(prover_job_output)).await.is_err() {
                        error!("Bridge Head API Error: Failed to send job success result - receiver dropped (system shutdown)");
                    }
                }
                Err(error) => {
                    let job_error = ProverJobError { job_id, error };
                    if tx.send(Err(job_error)).await.is_err() {
                        error!("Bridge Head API Error: Failed to send job error result - receiver dropped (system shutdown)");
                    }
                }
            }
        });

        // Here we should tell the finality_change_detector that we have a job inflight and its expected_output_slot
        // So it can begin preparing proof inputs from this input slot as well..
        self.finality_stage_input_tx
            .as_ref()
            .unwrap()
            .send(FinalityChangeDetectorUpdate {
                slot: expected_output_slot,
                store_hash: expected_output_store_hash,
                queue_cursor: expected_output_queue_cursor,
            })
            .await?;

        // Notify of a job created
        self.trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobCreated(
                TransitionNoticeExtensionBridgeHeadJobCreated {
                    input_slot: self.current_slot,
                    input_block_number: proof_inputs_with_window.input_block_number,
                    job_id,
                    expected_output_slot: self.next_slot,
                    expected_output_block_number: proof_inputs_with_window
                        .expected_output_block_number,
                    input_store_hash: store_hash,
                },
            ))
            .await?;

        Ok(())
    }

    /// Handle prover job success
    async fn handle_prover_success(
        &mut self,
        job_id: u64,
        proof: SP1ProofWithPublicValues,
    ) -> Result<()> {
        info!("Handling prover job output '{}'.", job_id);

        // Extract jobs details are remove job
        let (inputs_with_window, elapsed_sec) = {
            let job = self.prover_jobs.get(&job_id).unwrap();
            let inputs_with_window = job.inputs_with_window.clone();

            let elapsed_sec = Instant::now()
                .duration_since(job.start_instant)
                .as_secs_f64();

            self.prover_jobs.remove(&job_id);

            (inputs_with_window, elapsed_sec)
        };

        info!("Job '{}' finished in {} seconds.", job_id, elapsed_sec);

        // Extract values out of the proof output
        let public_values: sp1_sdk::SP1PublicValues = proof.clone().public_values;
        let public_values_bytes = public_values.as_slice(); // Raw bytes

        let proof_outputs = ProofOutputs::from_bytes(public_values_bytes)?;
        let input_slot = proof_outputs.input_slot;
        let input_store_hash = proof_outputs.input_store_hash;
        let output_slot = proof_outputs.output_slot;
        let output_store_hash = proof_outputs.output_store_hash;

        info!(
            "...proof_outputs.next_sync_committee_hash {}",
            proof_outputs.next_sync_committee_hash
        );

        info!(
            "PROOF OUTPUT SERIALIZED:\n{}",
            serde_json::to_string(&proof_outputs)?
        );
        info!("-----------------------------------------------------------------------------------------");
        info!("-----------------------------------------------------------------------------------------");
        info!("-----------------------------------------------------------------------------------------");

        // The committed leaf set, in cursor order. Every entry contributes a
        // leaf, so this is what a consumer rebuilds Merkle paths from.
        let queue_storage = &inputs_with_window.proof_inputs.queue_storage;
        let verified_requests: Vec<VerifiedRequest> = queue_storage
            .entries
            .iter()
            .map(|entry| VerifiedRequest {
                target: entry.target,
                collection_keys_count: entry.collection_keys_count,
                collection_keys: entry.collection_keys,
                value: queue_storage
                    .targets
                    .iter()
                    .find(|target| target.target_address == entry.target)
                    .and_then(|target| {
                        target
                            .slots
                            .iter()
                            .find(|slot| slot.key == entry.slot_key)
                            .map(|slot| slot.value)
                    })
                    .unwrap_or_default(),
            })
            .collect();

        // Notify of a successful job
        self.trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobSucceeded(
                TransitionNoticeExtensionBridgeHeadJobSucceeded {
                    input_slot,
                    input_block_number: inputs_with_window.input_block_number,
                    input_store_hash,
                    output_slot,
                    output_block_number: proof_outputs.output_block_number,
                    job_id,
                    elapsed_sec,
                    execution_state_root: proof_outputs.execution_state_root,
                    output_store_hash: proof_outputs.output_store_hash,
                    verified_contract_storage_slots_root: proof_outputs.verified_contract_storage_slots_root,
                    next_sync_committee_hash: proof_outputs.next_sync_committee_hash,
                    proof_request_queue_address: proof_outputs.proof_request_queue_address,
                    verified_requests: verified_requests.clone(),
                    input_queue_cursor: proof_outputs.input_queue_cursor,
                    output_queue_cursor: proof_outputs.output_queue_cursor,
                },
            ))
            .await?;

        // Emit proof
        self.trigger_listener_with_proof(ProofMessage {
                input_slot,
                input_block_number: inputs_with_window.input_block_number,
                input_store_hash,
                output_slot,
                output_block_number: proof_outputs.output_block_number,
                output_store_hash,
                proof,
                execution_state_root: proof_outputs.execution_state_root,
                verified_contract_storage_slots_root: proof_outputs.verified_contract_storage_slots_root,
                next_sync_committee_hash: proof_outputs.next_sync_committee_hash,
                proof_request_queue_address: proof_outputs.proof_request_queue_address,
                verified_requests,
                input_queue_cursor: proof_outputs.input_queue_cursor,
                output_queue_cursor: proof_outputs.output_queue_cursor,
                elapsed_sec,
            })
            .await?;

        Ok(())
    }

    /// Handle prover job failures
    async fn handle_prover_failure(
        &mut self,
        err: &ProverJobError,
    ) -> Result<(), mpsc::error::SendError<BridgeHeadEvent>> {
        // Extract job details and remove job
        let (inputs_with_window, n_jobs, elapsed_sec) = {
            let job = self.prover_jobs.get(&err.job_id).unwrap();
            let inputs_with_window = job.inputs_with_window.clone();

            let elapsed_sec = Instant::now()
                .duration_since(job.start_instant)
                .as_secs_f64();

            self.prover_jobs.remove(&err.job_id);

            (inputs_with_window, self.prover_jobs.len(), elapsed_sec)
        };

        // Build job failure error message
        let message = format!("Job '{}' failed with error: {}", err.job_id, err);
        error!("{}", message);

        // Notify of a job failure
        self.trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobFailed(
                TransitionNoticeExtensionBridgeHeadJobFailed {
                    input_slot: inputs_with_window.input_slot,
                    input_block_number: inputs_with_window.input_block_number,
                    input_store_hash: inputs_with_window.proof_inputs.store_hash,
                    expected_output_slot: inputs_with_window.expected_output_slot,
                    expected_output_block_number: inputs_with_window.expected_output_block_number,
                    job_id: err.job_id,
                    error: message,
                    elapsed_sec,
                    n_job_in_buffer: n_jobs as u64,
                },
            ))
            .await
    }

    // ================================================================================================
    // Commands
    // ================================================================================================

    // Advance the bridge head
    async fn advance(
        &mut self,
        slot: u64,
        store_hash: FixedBytes<32>,
    ) -> Result<(), mpsc::error::SendError<BridgeHeadEvent>> {
        // Update current head
        self.current_slot = slot;

        // Update the store hash
        self.store_hash = store_hash;

        // Notify of head advanced
        self.trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::HeadAdvanced(
                TransitionNoticeExtensionBridgeHeadAdvanced { slot, store_hash },
            ))
            .await?;

        Ok(())
    }

    // Update next slot logic
    async fn on_beacon_finality_change(
        &mut self,
        event: DualProofInputsWithWindow<MainnetConsensusSpec>,
    ) -> Result<(), mpsc::error::SendError<BridgeHeadEvent>> {
        
        // Unpack next_window (could be None)
        let next_window_proof_inputs_with_window =
            event.next_window.as_ref().map(|b| Box::new(b.clone()));

        // Notify of transition
        self.trigger_listener_with_notice(
                TransitionNoticeBridgeHeadMessageExtension::FinalityTransitionDetected(
                    TransitionNoticeExtensionBridgeHeadFinalityTransitionDetected {
                        block_number: event.current_window.expected_output_block_number,
                        slot: event.current_window.expected_output_slot,
                        input_slot: event.current_window.input_slot,
                        current_window_proof_inputs_with_window: Box::new(
                            event.current_window.clone(),
                        ),
                        next_window_proof_inputs_with_window,
                    },
                ),
            )
            .await?;

        // Update next head
        self.next_slot = event.current_window.expected_output_slot;

        // Print the head change detection
        info!("Beacon finality slot change detected. Current head is: '{}' Beacon finality head (next_head) is: '{}', Updating next_head.", self.current_slot, self.next_slot);

        Ok(())
    }

    // ================================================================================================
    // Event loop
    // ================================================================================================

    pub async fn run(mut self, current_slot: u64, store_hash: FixedBytes<32>, queue_cursor: u64, pipeline_inflight_next_expected_output: Option<FinalityChangeDetectorUpdate>) {
        // Extract the Sp1 config from envs and wrap it in an Arc so we can share it
        let sp1_config = Arc::new(
            ProverConfig::from_env()
                .expect("Failed to load a valid Sp1 config from env")
        );

        // Print the loaded configuration for user visibility
        sp1_config.print_config();

        // Construct the consensus proxy (includes execution proxy) from env
        let consensus_http_proxy = ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env();

        // Setup polling client for finality change detection
        info!("Starting finality change detector.");
        let (
            init_latest_beacon_slot,
            mut finality_output_rx,
            finality_advance_input_tx,
            finality_stage_input_tx,
        ) = start_validated_consensus_finality_change_detector::<MainnetConsensusSpec, HttpRpc>(
            consensus_http_proxy,
            current_slot,
            store_hash,
            queue_cursor,
            pipeline_inflight_next_expected_output,
        )
        .await;

        // Update current_slot and store_hash to init values
        self.current_slot = current_slot;
        self.store_hash = store_hash;

        // Move finality_stage_input_tx to self
        self.finality_stage_input_tx = Some(finality_stage_input_tx);

        // Copy init_latest_beacon_slot onto self
        self.next_slot = init_latest_beacon_slot;

        // Take the command and job rx from self
        let mut command_rx = self.command_rx.take().unwrap();
        let mut job_rx = self.job_rx.take().unwrap();

        self.trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::Started(
                TransitionNoticeExtensionBridgeHeadStarted {
                    latest_beacon_slot: init_latest_beacon_slot,
                    current_slot: self.current_slot,
                    store_hash: self.store_hash,
                },
            ))
            .await
            .expect("Failed to send Started event - observer receiver dropped");

        info!("Event loop started.");

        loop {
            tokio::select! {
                // Read the finality reciever for finality change events
                msg = finality_output_rx.recv() => match msg {
                    Some(event) => {
                        if let Err(err) = self.on_beacon_finality_change(event).await {
                            error!("Bridge Head API Error: Failed to send finality change event - observer receiver dropped: {:?}", err);
                            break;
                        }
                    }
                    None => {
                        error!("Bridge Head API Error: Finality change detector has dropped the output channel.");
                        break;
                    }
                },
                // Read the command receiver for input commands
                msg = command_rx.recv() => match msg {
                    Some(cmd) => {
                        match cmd {
                            Command::StageTransitionProof(message) => {
                                if let Err(err) = self.stage_transition_proof(Arc::clone(&sp1_config), *message).await {
                                    error!("Bridge Head API Error: Failed to stage transition proof: {:?}", err);
                                    break;
                                }
                            }
                            Command::Advance(message) => {
                                // Notify finality change detector of a change to the head position
                                // message.slot is the output slot which was finalised
                                if let Err(err) = finality_advance_input_tx.send(FinalityChangeDetectorUpdate {slot: message.slot, store_hash: message.store_hash, queue_cursor: message.queue_cursor}).await {
                                    error!("Bridge Head API Error: Failed to notify finality detector of head advancement: {:?}", err);
                                    break;
                                }
                                // Deal with advance invocation
                                if let Err(err) = self.advance(message.slot, message.store_hash).await {
                                    error!("Bridge Head API Error: Failed to send head advanced event - observer receiver dropped: {:?}", err);
                                    break;
                                }
                            }
                        }
                    }
                    None => {
                        error!("Bridge Head API Error: Command channel has been closed by handle holder.");
                        break;
                    }
                },
                // Read the job receiver for returned jobs
                msg = job_rx.recv() => match msg {
                    Some(job_result) => {
                        match job_result {
                            Ok(result_data) => {
                                let handle_prover_success_result = self.handle_prover_success(
                                    result_data.job_id(),
                                    result_data.proof(),
                                ).await;
                                if let Err(err) = handle_prover_success_result {
                                    error!("Bridge Head API Error: Error handling prover success: {:?}", err);
                                    break;
                                }
                            }
                            Err(err) => {
                                if let Err(send_err) = self.handle_prover_failure(&err).await {
                                    error!("Bridge Head API Error: Failed to send job failure event - observer receiver dropped: {:?}", send_err);
                                    break;
                                }
                            }
                        }
                    }
                    None => {
                        // Should never happen
                        error!("Bridge Head API Error: SP1 prover job result channel closed - all senders dropped. This should not happen since we hold self.job_tx.");
                        break;
                    }
                },
            }
        }
        error!(
            "Bridge Head API: event loop terminated. \
             Communication with finality detector, command sender, or prover workers has been lost."
        );
    }
}
