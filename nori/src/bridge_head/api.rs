use super::checkpoint::{load_nb_checkpoint, nb_checkpoint_exists, save_nb_checkpoint};
use super::finality_change_detector::{start_helios_finality_change_detector, FinalityChangeMessage};
use super::handles::{Command, CommandHandle};
use super::notice_messages::{
    BridgeHeadNoticeMessage, BridgeHeadNoticeMessageExtension, NoticeExtensionBridgeHeadAdvanced,
    NoticeExtensionBridgeHeadFinalityTransitionDetected, NoticeExtensionBridgeHeadJobCreated,
    NoticeExtensionBridgeHeadJobFailed, NoticeExtensionBridgeHeadJobSucceeded,
    NoticeExtensionBridgeHeadStarted,
};
use super::validate::validate_env;
use crate::helios::get_latest_finality_head;
use crate::proof_outputs_decoder::DecodedProofOutputs;
use crate::sp1_prover::{finality_update_job, ProverJobOutput};
use alloy_primitives::{FixedBytes, B256};
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
    pub output_slot: u64,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
    pub next_sync_committee: FixedBytes<32>,
    pub time_taken_seconds: f64,
}

struct ProverJob {
    start_instant: Instant,
    input_slot: u64,
    expected_output_slot: u64,
    next_sync_committee: FixedBytes<32>,
}

pub struct ProverJobError {
    pub job_idx: u64,
    pub error: Error,
}

// Implement Display trait for user-friendly error messages
impl fmt::Display for ProverJobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Prover job {} failed: {}", self.job_idx, self.error)
    }
}

// Implement Debug for ProverJobError
impl fmt::Debug for ProverJobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ProverJobError {{ job_idx: {}, source: {:?} }}",
            self.job_idx, self.error
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
    NoticeMessage(BridgeHeadNoticeMessage),
}

/// Core bridge head implementation that manages proof generation and state transitions
///
/// Handles:
/// - Proof generation workflow
/// - State management
/// - Event observation
pub struct BridgeHead {
    /// Current finalized slot head
    current_head: u64,
    /// Latest beacon slot when bridge head inited
    init_latest_beacon_slot: u64,
    /// Target slot to advance to
    next_slot: u64,
    /// Unique identifier for prover jobs
    job_idx: u64,
    /// Hash of the next sync committee
    next_sync_committee: FixedBytes<32>,
    /// Active prover jobs mapped by job ID
    prover_jobs: HashMap<u64, ProverJob>,
    /// Channel for receiving bridge head commands
    command_rx: Option<mpsc::Receiver<Command>>,
    /// Chanel for receiving helios finality transition events
    finality_rx: Option<mpsc::Receiver<FinalityChangeMessage>>,
    /// Channel for receiving job results
    job_rx: Option<mpsc::UnboundedReceiver<Result<ProverJobOutput, ProverJobError>>>,
    /// Channel for sending job results
    job_tx: mpsc::UnboundedSender<Result<ProverJobOutput, ProverJobError>>,
    /// Channel for emitting proof and notice events
    event_tx: broadcast::Sender<BridgeHeadEvent>,
    /// Boolean for indicating a job should be issued on the next beacon slot change event
    stage_transition_proof: bool,
    /// Boolean indicating a cold start (prevents waiting for transition)
    cold_start: bool,
}

impl BridgeHead {
    pub async fn new() -> (CommandHandle, Self) {
        validate_env(&["SOURCE_CONSENSUS_RPC_URL", "SP1_PROVER"]);

        // Initialise slot head / commitee vars
        let current_head;
        let mut next_sync_committee = FixedBytes::<32>::default();
        let cold_start;
        // Start procedure
        if nb_checkpoint_exists() {
            // Warm start procedure
            info!("Loading nori slot checkpoint from file.");
            let nb_checkpoint = load_nb_checkpoint().unwrap();
            current_head = nb_checkpoint.slot_head;
            next_sync_committee = nb_checkpoint.next_sync_committee;
            cold_start = false;
        } else {
            // Cold start procedure
            // FIXME we should be going from a trusted checkpoint TODO
            info!("Resorting to cold start procedure.");
            current_head = get_latest_finality_head().await.unwrap();
            cold_start = true;
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
            start_helios_finality_change_detector(current_head).await;

        (
            input_command_handle,
            BridgeHead {
                current_head,
                init_latest_beacon_slot,
                next_slot: init_latest_beacon_slot,
                job_idx: 0,
                next_sync_committee,
                prover_jobs: HashMap::new(),
                command_rx: Some(command_rx),
                finality_rx: Some(finality_rx),
                job_rx: Some(job_rx),
                job_tx,
                event_tx,
                stage_transition_proof: false,
                cold_start
            }
        )
    }

    /// Event dispatchers

    // Event receiver

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
        extension: BridgeHeadNoticeMessageExtension,
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
        input_head: u64,
        proof: SP1ProofWithPublicValues,
        job_idx: u64,
        elapsed_sec: f64,
    ) -> Result<()> {
        info!("Handling prover job output '{}'.", job_idx);

        // Remove completed job
        self.prover_jobs.remove(&job_idx);

        // Extract the next sync committee out of the proof output
        let public_values: sp1_sdk::SP1PublicValues = proof.clone().public_values;
        let public_values_bytes = public_values.as_slice(); // Raw bytes
        let proof_outputs = DecodedProofOutputs::from_abi(public_values_bytes)?;

        // need to cast new head to a u64 // FIXME change the decoder later.......
        let new_head = proof_outputs.new_head;
        let new_head_bytes: [u8; 32] = new_head.to_be_bytes();
        // Extract the last 8 bytes (least significant bytes in big-endian)
        let new_head_u64_bytes: [u8; 8] = new_head_bytes[24..32]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to extract u64 bytes"))?;
        // Convert the bytes to u64 using big-endian interpretation
        let output_head = u64::from_be_bytes(new_head_u64_bytes);

        // Notify of a succesful job
        let _ = self
            .trigger_listener_with_notice(BridgeHeadNoticeMessageExtension::JobSucceeded(
                NoticeExtensionBridgeHeadJobSucceeded {
                    input_slot: input_head,
                    output_slot: output_head,
                    job_idx,
                    next_sync_committee: proof_outputs.next_sync_committee_hash,
                    elapsed_sec,
                    execution_state_root: proof_outputs.execution_state_root,
                },
            ))
            .await;

        // Emit proof
        let _ = self
            .trigger_listener_with_proof(ProofMessage {
                input_slot: input_head,
                output_slot: output_head,
                proof,
                execution_state_root: proof_outputs.execution_state_root,
                next_sync_committee: proof_outputs.next_sync_committee_hash,
                time_taken_seconds: elapsed_sec,
            })
            .await;

        Ok(())
    }

    // Handle prover job failures
    async fn handle_prover_failure(&mut self, err: &ProverJobError) {
        // Cache the fields needed from the job so we can release the immutable borrow.
        let (input_slot, expected_output_slot, n_jobs, elapsed_sec) = {
            // Immutable borrow to get the job.
            let job = self.prover_jobs.get(&err.job_idx).unwrap();
            let input_slot = job.input_slot;
            let expected_output_slot = job.expected_output_slot;

            // Elapsed
            let elapsed_sec = Instant::now()
                .duration_since(job.start_instant)
                .as_secs_f64();

            self.prover_jobs.remove(&err.job_idx);

            (
                input_slot,
                expected_output_slot,
                self.prover_jobs.len(),
                elapsed_sec,
            )
        };

        let message = format!("Job '{}' failed with error: {}", err.job_idx, err);
        error!("Job '{}' failed with error: {}", err.job_idx, err);

        let _ = self
            .trigger_listener_with_notice(BridgeHeadNoticeMessageExtension::JobFailed(
                NoticeExtensionBridgeHeadJobFailed {
                    input_slot,
                    expected_output_slot,
                    job_idx: err.job_idx,
                    message,
                    elapsed_sec,
                    n_job_in_buffer: n_jobs as u64,
                },
            ))
            .await;
    }

    // Create prover job
    async fn prepare_transition_proof(&mut self) {
        self.job_idx += 1;
        let job_idx: u64 = self.job_idx;
        info!(
            "Nori head updater received a new job {}. Spawning a new worker.",
            job_idx
        );

        self.prover_jobs.insert(
            job_idx,
            ProverJob {
                start_instant: Instant::now(),
                next_sync_committee: self.next_sync_committee,
                input_slot: self.current_head,
                expected_output_slot: self.next_slot,
            },
        );

        let tx = self.job_tx.clone();
        let current_head_clone = self.current_head;
        let next_sync_committee_clone = self.next_sync_committee;

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            let proof_result =
                finality_update_job(job_idx, current_head_clone, next_sync_committee_clone).await;

            match proof_result {
                Ok(prover_job_output) => {
                    tx.send(Ok(prover_job_output)).unwrap();
                }
                Err(error) => {
                    let job_error = ProverJobError { job_idx, error };
                    tx.send(Err(job_error)).unwrap();
                }
            }
        });

        let _ = self
            .trigger_listener_with_notice(BridgeHeadNoticeMessageExtension::JobCreated(
                NoticeExtensionBridgeHeadJobCreated {
                    input_slot: self.current_head,
                    job_idx,
                    expected_output_slot: self.next_slot,
                },
            ))
            .await;
    }

    /// Commands

    // Stage transition proof generation
    async fn stage_transition_proof(&mut self) {
        if self.cold_start {
            // If we started cold run the first job regardless of if our next_slot = current_head
            let _ = self.prepare_transition_proof().await;
            self.cold_start = false;
            return;
        }
        
        if self.next_slot > self.current_head {
            // Immediately do the transition proof job.
            let _ = self.prepare_transition_proof().await;
            self.stage_transition_proof = false;
        } else {
            // Wait for finality transition before doing the transition proof job.
            self.stage_transition_proof = true;
        }
    }

    // Advance the bridge head
    async fn advance(&mut self, head: u64, next_sync_committee: FixedBytes<32>) {
        // Update current head
        self.current_head = head;

        // If sync committee is valid then update it
        if next_sync_committee != B256::ZERO {
            // ONLY IF NON ZEROS
            self.next_sync_committee = next_sync_committee;
        }

        // Save the checkpoint
        info!("Saving checkpoint.");
        save_nb_checkpoint(self.current_head, self.next_sync_committee);

        // Notify
        let _ = self
            .trigger_listener_with_notice(BridgeHeadNoticeMessageExtension::HeadAdvanced(
                NoticeExtensionBridgeHeadAdvanced {
                    head,
                    next_sync_committee,
                },
            ))
            .await;
    }

    // Update next slot logic
    async fn on_beacon_finality_change(&mut self, slot: u64) {
        // Notify of transition
        let _ = self
            .trigger_listener_with_notice(
                BridgeHeadNoticeMessageExtension::FinalityTransitionDetected(
                    NoticeExtensionBridgeHeadFinalityTransitionDetected { slot },
                ),
            )
            .await;

        // Update next head
        self.next_slot = slot;

        // Print the head change detection
        info!("Helios beacon finality slot change detected. Current head is: '{}' Beacon finality head (next_head) is: '{}', Updating next_head.", self.current_head, self.next_slot);

        // If we have a staged transition proof then attempt to run that job
        if self.stage_transition_proof {
            let _ = self.stage_transition_proof().await;
        }
    }

    /// Event loop

    pub async fn run(mut self) {
        let _ = self
            .trigger_listener_with_notice(BridgeHeadNoticeMessageExtension::Started(
                NoticeExtensionBridgeHeadStarted {
                    latest_beacon_slot: self.init_latest_beacon_slot,
                    current_head: self.current_head,
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
                    let _ = self.on_beacon_finality_change(event.slot).await;
                }
                // Read the command receiver for input commands
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        Command::StageTransitionProof => {
                            let _ = self.stage_transition_proof().await;
                        }
                        Command::Advance(message) => {
                            // deal with advance invocation
                            let _ = self.advance(message.head, message.next_sync_committee).await;
                        }
                    }
                }
                // Read the job receiver for returned jobs
                Some(job_result) = job_rx.recv() => {
                    match job_result {
                        Ok(result_data) => {
                            let job: &ProverJob = self.prover_jobs.get(&result_data.job_idx()).unwrap();
                            let time_taken_seconds = Instant::now()
                                .duration_since(job.start_instant)
                                .as_secs_f64();

                            info!(
                                "Job '{}' finished in {} seconds.",
                                result_data.job_idx(), time_taken_seconds
                            );

                            let _ = self.handle_prover_success(
                                result_data.input_head(),
                                result_data.proof(),
                                result_data.job_idx(),
                                time_taken_seconds,
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
