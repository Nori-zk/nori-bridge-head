use super::checkpoint::{load_nb_checkpoint, nb_checkpoint_exists, save_nb_checkpoint};
use super::finality_change_detector::start_helios_finality_change_detector;
use super::handles::{Command, CommandHandle};
use super::notice_messages::{
    TransitionNoticeBridgeHeadMessage, TransitionNoticeBridgeHeadMessageExtension, TransitionNoticeExtensionBridgeHeadAdvanced,
    TransitionNoticeNoticeExtensionBridgeHeadFinalityTransitionDetected, TransitionNoticeExtensionBridgeHeadJobCreated,
    TransitionNoticeExtensionBridgeHeadJobFailed, TransitionNoticeExtensionBridgeHeadJobSucceeded,
    TransitionNoticeExtensionBridgeHeadStarted,
};
use super::validate::validate_env;
use crate::helios::{get_latest_finality_slot_and_store_hash, FinalityUpdateAndSlot};
use crate::proof_outputs_decoder::DecodedProofOutputs;
use crate::sp1_prover::{finality_update_job, ProverJobOutput};
use alloy_primitives::FixedBytes;
use anyhow::{Error, Result};
use chrono::{SecondsFormat, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use sp1_sdk::SP1ProofWithPublicValues;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Proof types

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProofMessage {
    pub input_slot: u64,
    pub input_store_hash: FixedBytes<32>,
    pub output_slot: u64,
    pub output_store_hash: FixedBytes<32>,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
    pub elapsed_sec: f64,
}

struct ProverJob {
    input_slot: u64,
    input_store_hash: FixedBytes<32>,
    start_instant: Instant,
    expected_output_slot: u64,
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
    /// Latest beacon slot when bridge head inited
    init_latest_beacon_slot: u64,
    /// Target slot to advance to
    next_slot: u64,
    /// Unique identifier for prover jobs
    job_id: u64,
    /// Active prover jobs mapped by job ID
    prover_jobs: HashMap<u64, ProverJob>,
    /// Channel for receiving bridge head commands
    command_rx: Option<mpsc::Receiver<Command>>,
    /// Chanel for receiving helios finality transition events
    finality_rx: Option<mpsc::Receiver<FinalityUpdateAndSlot>>,
    /// Channel for receiving job results
    job_rx: Option<mpsc::UnboundedReceiver<Result<ProverJobOutput, ProverJobError>>>,
    /// Channel for sending job results
    job_tx: mpsc::UnboundedSender<Result<ProverJobOutput, ProverJobError>>,
    /// Channel for emitting proof and notice events
    event_tx: broadcast::Sender<BridgeHeadEvent>,
    /// FixedBytes representing the store hash
    store_hash: FixedBytes<32>,
}

impl BridgeHead {
    pub async fn new() -> (CommandHandle, Self) {
        validate_env(&["SOURCE_CONSENSUS_RPC_URL", "SP1_PROVER"]);

        // Initialise slot head / commitee vars
        let current_slot;
        let store_hash;

        // Start procedure
        if nb_checkpoint_exists() {
            // Warm start procedure
            info!("Loading nori slot checkpoint from file.");
            let nb_checkpoint = load_nb_checkpoint().unwrap();
            current_slot = nb_checkpoint.slot;
            store_hash = nb_checkpoint.store_hash;
        } else {
            // Cold start procedure
            // FIXME we should be going from a trusted checkpoint TODO
            info!("Resorting to cold start procedure.");
            (current_slot, store_hash) = get_latest_finality_slot_and_store_hash().await.unwrap();
        }

        // Setup command mpsc
        let (command_tx, command_rx) = mpsc::channel(2); // FIXME this isnt the best choice of buffer size. It makes assumptions that the sender knows what they are doing.

        // Create command handle
        let input_command_handle = CommandHandle::new(command_tx);

        // Create job mpsc
        let (job_tx, job_rx) = mpsc::unbounded_channel();

        // Create events broadcast chanel
        let (event_tx, _) = broadcast::channel(16);

        // Setup polling client for finality change detection
        info!("Starting helios polling client.");
        let (init_latest_beacon_slot, finality_rx) =
            start_helios_finality_change_detector(current_slot).await;

        (
            input_command_handle,
            BridgeHead {
                current_slot,
                init_latest_beacon_slot,
                next_slot: init_latest_beacon_slot,
                job_id: 0,
                prover_jobs: HashMap::new(),
                command_rx: Some(command_rx),
                finality_rx: Some(finality_rx),
                job_rx: Some(job_rx),
                job_tx,
                event_tx,
                store_hash,
            },
        )
    }

    /// Event dispatchers

    // Create event job receiver
    pub fn event_receiver(&self) -> broadcast::Receiver<BridgeHeadEvent> {
        self.event_tx.subscribe()
    }

    //  Emit proofs
    async fn trigger_listener_with_proof(&mut self, payload: ProofMessage) -> Result<()> {
        let _ = self.event_tx.send(BridgeHeadEvent::ProofMessage(payload));
        Ok(())
    }

    // Emit notices
    async fn trigger_listener_with_notice(
        &mut self,
        extension: TransitionNoticeBridgeHeadMessageExtension,
    ) -> Result<()> {
        let now = Utc::now();
        let iso_string = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let notice_message = extension.into_message(iso_string);
        let _ = self
            .event_tx
            .send(BridgeHeadEvent::NoticeMessage(notice_message));
        Ok(())
    }

    /// Jobs Handlers

    // Handle prover job success
    async fn handle_prover_success(
        &mut self,
        job_id: u64,
        proof: SP1ProofWithPublicValues,
    ) -> Result<()> {
        info!("Handling prover job output '{}'.", job_id);

        // Extract jobs details are remove job
        let (input_slot, input_store_hash, elapsed_sec) = {
            let job = self.prover_jobs.get(&job_id).unwrap();
            let input_slot = job.input_slot;
            let input_store_hash = job.input_store_hash;

            let elapsed_sec = Instant::now()
                .duration_since(job.start_instant)
                .as_secs_f64();

            self.prover_jobs.remove(&job_id);

            (
                input_slot,
                input_store_hash,
                elapsed_sec,
            )
        };

        info!("Job '{}' finished in {} seconds.", job_id, elapsed_sec);

        // Extract the next sync committee out of the proof output
        let public_values: sp1_sdk::SP1PublicValues = proof.clone().public_values;
        let public_values_bytes = public_values.as_slice(); // Raw bytes
        let proof_outputs = DecodedProofOutputs::from_abi(public_values_bytes)?;

        // need to cast new head to a u64 // FIXME change the decoder later.......
        let new_slot = proof_outputs.new_head;
        let new_slot_bytes: [u8; 32] = new_slot.to_be_bytes();
        // Extract the last 8 bytes (least significant bytes in big-endian)
        let new_slot_u64_bytes: [u8; 8] = new_slot_bytes[24..32]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to extract u64 bytes"))?;
        // Convert the bytes to u64 using big-endian interpretation
        let output_slot = u64::from_be_bytes(new_slot_u64_bytes);
        // Get the store hash
        let output_store_hash = proof_outputs.store_hash;

        // Notify of a succesful job
        let _ = self
            .trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobSucceeded(
                TransitionNoticeExtensionBridgeHeadJobSucceeded {
                    input_slot,
                    input_store_hash,
                    output_slot,
                    job_id,
                    elapsed_sec,
                    execution_state_root: proof_outputs.execution_state_root,
                    output_store_hash: proof_outputs.store_hash,
                },
            ))
            .await;

        // Emit proof
        let _ = self
            .trigger_listener_with_proof(ProofMessage {
                input_slot,
                input_store_hash,
                output_slot,
                output_store_hash,
                proof,
                execution_state_root: proof_outputs.execution_state_root,
                elapsed_sec,
            })
            .await;

        Ok(())
    }

    // Handle prover job failures
    async fn handle_prover_failure(&mut self, err: &ProverJobError) {
        // Extract job details and remove job
        let (input_slot, input_store_hash, expected_output_slot, n_jobs, elapsed_sec) = {
            let job = self.prover_jobs.get(&err.job_id).unwrap();
            let input_slot = job.input_slot;
            let input_store_hash = job.input_store_hash;
            let expected_output_slot = job.expected_output_slot;

            let elapsed_sec = Instant::now()
                .duration_since(job.start_instant)
                .as_secs_f64();

            self.prover_jobs.remove(&err.job_id);

            (
                input_slot,
                input_store_hash,
                expected_output_slot,
                self.prover_jobs.len(),
                elapsed_sec,
            )
        };

        // Build job failure error message
        let message = format!("Job '{}' failed with error: {}", err.job_id, err);
        error!("{}", message);

        // Notify of a job failure
        let _ = self
            .trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobFailed(
                TransitionNoticeExtensionBridgeHeadJobFailed {
                    input_slot,
                    input_store_hash,
                    expected_output_slot,
                    job_id: err.job_id,
                    message,
                    elapsed_sec,
                    n_job_in_buffer: n_jobs as u64,
                },
            ))
            .await;
    }

    // Create prover job
    async fn prepare_transition_proof(&mut self, slot: u64, store_hash: FixedBytes<32>) {
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
                input_slot: slot,
                input_store_hash: store_hash,
                start_instant: Instant::now(),
                expected_output_slot: self.next_slot,
            },
        );

        // Create job data tx
        let tx = self.job_tx.clone();

        // Clone job arguments
        let current_slot_clone = self.current_slot;
        let store_hash = self.store_hash;

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            // Execute job
            let proof_result = finality_update_job(
                job_id,
                current_slot_clone,
                store_hash,
            )
            .await;

            // Send appropriate tx Ok or Err
            match proof_result {
                Ok(prover_job_output) => {
                    tx.send(Ok(prover_job_output)).unwrap();
                }
                Err(error) => {
                    let job_error = ProverJobError { job_id, error };
                    tx.send(Err(job_error)).unwrap();
                }
            }
        });

        // Notify of a job created
        let _ = self
            .trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::JobCreated(
                TransitionNoticeExtensionBridgeHeadJobCreated {
                    input_slot: self.current_slot,
                    job_id,
                    expected_output_slot: self.next_slot,
                    input_store_hash: store_hash,
                },
            ))
            .await;
    }

    /// Commands

    // Advance the bridge head
    async fn advance(
        &mut self,
        slot: u64,
        store_hash: FixedBytes<32>,
    ) {
        // Update current head
        self.current_slot = slot;

        // Update the store has
        self.store_hash = store_hash;

        // Save the checkpoint
        save_nb_checkpoint(self.current_slot, self.store_hash);

        // Notify of head advanced
        let _ = self
            .trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::HeadAdvanced(
                TransitionNoticeExtensionBridgeHeadAdvanced {
                    slot,
                    store_hash,
                },
            ))
            .await;
    }

    // Update next slot logic
    async fn on_beacon_finality_change(&mut self, event: FinalityUpdateAndSlot) {
        // Notify of transition
        let _ = self
            .trigger_listener_with_notice(
                TransitionNoticeBridgeHeadMessageExtension::FinalityTransitionDetected(
                    TransitionNoticeNoticeExtensionBridgeHeadFinalityTransitionDetected { slot: event.slot, finality_update: event.finality_update },
                ),
            )
            .await;

        // Update next head
        self.next_slot = event.slot;

        // Print the head change detection
        info!("Helios beacon finality slot change detected. Current head is: '{}' Beacon finality head (next_head) is: '{}', Updating next_head.", self.current_slot, self.next_slot);
    }

    /// Event loop

    pub async fn run(mut self) {
        let _ = self
            .trigger_listener_with_notice(TransitionNoticeBridgeHeadMessageExtension::Started(
                TransitionNoticeExtensionBridgeHeadStarted {
                    latest_beacon_slot: self.init_latest_beacon_slot,
                    current_slot: self.current_slot,
                    store_hash: self.store_hash,
                },
            ))
            .await;

        info!("Event loop started.");

        let mut finality_rx = self.finality_rx.take().unwrap();
        let mut command_rx = self.command_rx.take().unwrap();
        let mut job_rx = self.job_rx.take().unwrap();

        loop {
            tokio::select! {
                // Read the finality reciever for finality change events
                Some(event) = finality_rx.recv() => {
                    let _ = self.on_beacon_finality_change(event).await;
                }
                // Read the command receiver for input commands
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        Command::StageTransitionProof(message) => {
                            let _ = self.prepare_transition_proof(message.slot, message.store_hash).await;
                        }
                        Command::Advance(message) => {
                            // deal with advance invocation
                            let _ = self.advance(message.slot, message.store_hash).await;
                        }
                    }
                }
                // Read the job receiver for returned jobs
                Some(job_result) = job_rx.recv() => {
                    match job_result {
                        Ok(result_data) => {
                            let _ = self.handle_prover_success(
                                result_data.job_id(),
                                result_data.proof(),
                            ).await;
                        }
                        Err(err) => {
                            let _ = self.handle_prover_failure(&err).await;
                        }
                    }
                }
            }
        }
    }
}
