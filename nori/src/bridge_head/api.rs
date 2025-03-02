use super::checkpoint::{load_nb_checkpoint, nb_checkpoint_exists, save_nb_checkpoint};
use super::handles::{AdvanceHandle, BeaconFinalityChangeHandle, EventLoopCommand};
use super::notice_messages::{
    get_notice_message_type, NoticeAdvanceRequested, NoticeBaseMessage,
    NoticeFinalityTransitionDetected, NoticeHeadAdvanced, NoticeJobCreated, NoticeJobFailed,
    NoticeJobSucceeded, NoticeMessage, NoticeMessageExtension, NoticeStarted,
};
use crate::helios::get_latest_finality_head;
use crate::proof_outputs_decoder::DecodedProofOutputs;
use crate::sp1_prover::{finality_update_job, ProverJobOutput};
use alloy_primitives::FixedBytes;
use anyhow::{Error, Result};
use chrono::{SecondsFormat, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use sp1_sdk::SP1ProofWithPublicValues;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::sync::broadcast;

const TYPICAL_FINALITY_TRANSITION_TIME: f64 = 384.0;

/// Proof types
#[derive(Serialize, Deserialize, Clone)]
pub struct ProofMessage {
    pub slot: u64,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
    pub next_sync_committee: FixedBytes<32>,
    pub time_taken_second: f64,
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

struct ProverJob {
    start_instant: Instant,
    slot: u64,
    next_sync_committee: FixedBytes<32>,
}

// Event enum
#[derive(Clone)]
pub enum BridgeHeadEvent {
    ProofMessage(ProofMessage),
    NoticeMessage(NoticeMessage)
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
    /// Target slot to advance to
    next_head: u64,
    /// Slot currently being processed
    working_head: u64,
    /// Last beacon finality update checked
    last_beacon_finality_head_checked: u64,
    /// Job index for indicating if the job is set to auto advance
    auto_advance_index: u64,
    /// Unique identifier for prover jobs
    job_idx: u64,
    /// Duration of last completed job in seconds
    last_job_duration_sec: f64,
    /// Clocktime of last finality transition
    last_finality_transition_instant: Instant,
    /// Hash of the next sync committee
    next_sync_committee: FixedBytes<32>,
    /// Active prover jobs mapped by job ID
    prover_jobs: HashMap<u64, ProverJob>,
    /// Channel for receiving bridge head commands
    command_rx: Option<mpsc::Receiver<EventLoopCommand>>,
    /// Channel for receiving job results
    job_rx: Option<mpsc::UnboundedReceiver<Result<ProverJobOutput, ProverJobError>>>,
    /// Channel for sending job results
    job_tx: mpsc::UnboundedSender<Result<ProverJobOutput, ProverJobError>>,
    /// Channel for emitting proof and notice events
    event_tx: broadcast::Sender<BridgeHeadEvent>
}

impl BridgeHead {
    pub async fn new() -> (u64, AdvanceHandle, BeaconFinalityChangeHandle, Self) {
        // Initialise slot head / commitee vars
        let current_head;
        let mut next_sync_committee = FixedBytes::<32>::default();

        // Start procedure
        if nb_checkpoint_exists() {
            // Warm start procedure
            info!("Loading nori slot checkpoint from file.");
            let nb_checkpoint = load_nb_checkpoint().unwrap();
            current_head = nb_checkpoint.slot_head;
            next_sync_committee = nb_checkpoint.next_sync_committee;
        } else {
            // Cold start procedure
            info!("Resorting to cold start procedure.");
            current_head = get_latest_finality_head().await.unwrap();
        }

        // Create command mpsc
        let (command_tx, command_rx) = mpsc::channel(2);

        // Create job mpsc
        let (job_tx, job_rx) = mpsc::unbounded_channel();

        // Create events broadcast chanel
        let (event_tx, _) = broadcast::channel(16);

        (
            current_head,
            AdvanceHandle::new(command_tx.clone()),
            BeaconFinalityChangeHandle::new(command_tx),
            BridgeHead {
                current_head,
                next_head: current_head,
                working_head: current_head,
                last_beacon_finality_head_checked: u64::default(),
                auto_advance_index: 1,
                job_idx: 1,
                last_job_duration_sec: 0.0,
                last_finality_transition_instant: Instant::now(),
                next_sync_committee,
                prover_jobs: HashMap::new(),
                command_rx: Some(command_rx),
                job_rx: Some(job_rx),
                job_tx,
                event_tx,
            },
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
        extension: NoticeMessageExtension,
    ) -> Result<()> {
        let now = Utc::now();
        let iso_string = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let message_type = get_notice_message_type(&extension);
        let base_message = NoticeBaseMessage {
            timestamp: iso_string,
            message_type,
            current_head: self.current_head,
            next_head: self.next_head,
            working_head: self.working_head,
            last_beacon_finality_head_checked: self.last_beacon_finality_head_checked,
            last_job_duration_seconds: self.last_job_duration_sec,
            time_until_next_finality_transition_seconds: Instant::now()
                .duration_since(self.last_finality_transition_instant)
                .as_secs_f64(),
        };
        let full_message = NoticeMessage {
            base: base_message,
            extension,
        };
        let _ = self.event_tx.send(BridgeHeadEvent::NoticeMessage(full_message));
        Ok(())
    }

    /// Jobs

    // Create prover job
    async fn create_prover_job(
        &mut self,
        job_idx: u64,
        slot: u64,
        next_sync_committee: FixedBytes<32>,
    ) {
        info!(
            "Nori head updater recieved a new job {}. Spawning a new worker.",
            job_idx
        );

        self.working_head = slot; // Mark the working head as what was given by the slot

        //let last_next_sync_committee = self.next_sync_committee;
        let slot_clone = slot;

        self.prover_jobs.insert(
            job_idx,
            ProverJob {
                start_instant: Instant::now(),
                next_sync_committee,
                slot,
            },
        );

        let tx = self.job_tx.clone();

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            let proof_result = finality_update_job(job_idx, slot_clone, next_sync_committee).await;

            match proof_result {
                Ok(prover_job_output) => {
                    tx.send(Ok(prover_job_output)).unwrap();
                }
                Err(error) => {
                    let job_error = ProverJobError {
                        job_idx,
                        error,
                    };
                    tx.send(Err(job_error)).unwrap();
                }
            }
        });

        /*
           Need to carefully deactivate auto_advance, but lets think about this....
           We might have had advance called multiple times we need some concept of the job number to know if we should prevent auto advancement.
           If this task was the last auto advance task we should cancel that behaviour
        */
        if job_idx >= self.auto_advance_index && job_idx != 0 { // if its already zero we dont need to do this FIXME
            // Do we need the if... this should always be true CHECKME
            // gt to account for if a previous job spawned failed with an error and didnt cancel itself
            info!(
                "Cancelling auto advance for job '{}'.",
                self.auto_advance_index
            );
            self.auto_advance_index = 0;
        }

        let _ =
            self.trigger_listener_with_notice(NoticeMessageExtension::JobCreated(
                NoticeJobCreated { slot, job_idx },
            ))
            .await;
    }

    // Handle prover job success
    async fn handle_prover_success(
        &mut self,
        slot: u64,
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

        // Notify of a succesful job
        let _ = self
            .trigger_listener_with_notice(NoticeMessageExtension::JobSucceeded(
                NoticeJobSucceeded {
                    slot,
                    job_idx,
                    next_sync_committee: proof_outputs.next_sync_committee_hash,
                },
            ))
            .await;

        // Check if our result is still relevant after the computation, if our working_head has advanced then we are working on a more recent head in another thread.
        if self.working_head == slot {
            // could move this to a job_idx check vs auto_advance_index if we really wanted to skip what we've got in place with advance being called.
            // Update our state
            info!("Moving nori head forward.");

            // Update head and next_sync_committee based on the proof outputs
            self.current_head = slot;
            if proof_outputs.next_sync_committee_hash != FixedBytes::<32>::default() {
                self.next_sync_committee = proof_outputs.next_sync_committee_hash;
                // But wait! We need to check the logic in SP1Helios.sol as this can be Zeros
            }
            // Save the checkpoint
            info!("Saving checkpoint.");
            save_nb_checkpoint(self.current_head, self.next_sync_committee);

            info!("Triggering proof listeners");
            // Emit proof
            let _ = self
                .trigger_listener_with_proof(ProofMessage {
                    slot,
                    proof,
                    execution_state_root: proof_outputs.execution_state_root,
                    next_sync_committee: proof_outputs.next_sync_committee_hash,
                    time_taken_second: elapsed_sec,
                })
                .await;

            // Notify of head advance
            let _ = self
                .trigger_listener_with_notice(NoticeMessageExtension::HeadAdvanced(
                    NoticeHeadAdvanced {
                        slot,
                        next_sync_committee: self.next_sync_committee,
                    },
                ))
                .await;

            if self.next_head == self.current_head {
                info!(
                    "Nori bridge head is up to date. Current head is '{}'.",
                    self.current_head
                );
            }
        }
        Ok(())
    }

    // Handle prover job failures
    async fn handle_prover_failure(&mut self, err: &ProverJobError) {
        // Cache the fields needed from the job so we can release the immutable borrow.
        let (slot, next_sync_committee) = {
            // Immutable borrow to get the job.
            let job = self.prover_jobs.get(&err.job_idx).unwrap();
            (job.slot, job.next_sync_committee)
        };

        let message = format!("Job '{}' failed with error: {}", err.job_idx, err);
        error!("Job '{}' failed with error: {}", err.job_idx, err);

        let _ = self
        .trigger_listener_with_notice(NoticeMessageExtension::JobFailed(NoticeJobFailed {
            slot,
            job_idx: err.job_idx,
            message,
        }))
        .await;

        // Now that we've released the immutable borrow, we can mutably borrow `self`.
        let _ = self
            .create_prover_job(err.job_idx, slot, next_sync_committee)
            .await;
    }

    /// Commands

    // advance
    async fn advance(&mut self) {
        let _ = self
            .trigger_listener_with_notice(NoticeMessageExtension::AdvanceRequested(
                NoticeAdvanceRequested {},
            ))
            .await;

        // FIXME extract most of this logic out into a 'strategy'

        self.job_idx += 1;
        info!("Advance called ready for job '{}'.", self.job_idx);
        if self.next_head > self.current_head {
            // TODO think about the time until the next transition
            if self.last_job_duration_sec != 0.0 {
                // If we have data on the last job time
                if self.last_job_duration_sec > TYPICAL_FINALITY_TRANSITION_TIME {
                    warn!("Long last job duration '{}' seconds, compared to the typical finality transition time '{}'. If this continues nori bridge will definitely not be able to catch up.", self.last_job_duration_sec, TYPICAL_FINALITY_TRANSITION_TIME);
                } else if self.last_job_duration_sec > TYPICAL_FINALITY_TRANSITION_TIME * 0.8 {
                    warn!("Long last job duration '{}' seconds, compared to the typical finality transition time '{}'. If this continues nori bridge will take a while to catch up.", self.last_job_duration_sec, TYPICAL_FINALITY_TRANSITION_TIME);
                }

                let seconds_since_last_transition = Instant::now()
                    .duration_since(self.last_finality_transition_instant)
                    .as_secs_f64();

                info!(
                    "Expected time to next finality transition is {} seconds.",
                    TYPICAL_FINALITY_TRANSITION_TIME - seconds_since_last_transition
                );

                if seconds_since_last_transition > 0.8 * TYPICAL_FINALITY_TRANSITION_TIME {
                    warn!("We are within 80% of the typical finality transition time away from the next finality transition. Strategically waiting for the next finality transition in order to try to catch up more quickly. Note this will cause latency for current transactions in the bridge.");
                    // We should wait for the next detected head update.
                    self.auto_advance_index = self.job_idx;
                }
            }

            // We should immediately try to create a new proof
            if self.auto_advance_index != self.job_idx {
                info!("Immediately trying to advance.");
                let _ = self
                    .create_prover_job(self.job_idx, self.next_head, self.next_sync_committee)
                    .await;
            }
        } else {
            info!("Setting flag to attempt auto advance head on next finality update, as nori bridge is currently up to date.");
            // We should wait for the next detected head update.
            self.auto_advance_index = self.job_idx;
        }
    }

    async fn on_beacon_finality_change(&mut self, next_head: u64) {
        // We have a transition!

        // Notify of transition
        let _ = self
            .trigger_listener_with_notice(NoticeMessageExtension::FinalityTransitionDetected(
                NoticeFinalityTransitionDetected { slot: next_head },
            ))
            .await;

        // Update next head
        self.next_head = next_head;
        // Print the head change detection
        self.last_finality_transition_instant = Instant::now();
        info!("Helios beacon finality slot change detected. Nori bridge head is stale. Current head is: '{}' Working head is '{}' Beacon finality head (next_head) is: '{}', Updating next_head.", self.current_head, self.working_head, self.next_head);

        // Auto advance if nessesary
        if self.auto_advance_index != 0 {
            info!("Auto advance invoking job due to finality transition.");
            // Invoke a job
            let _ = self
                .create_prover_job(self.job_idx, self.next_head, self.next_sync_committee)
                .await;
        }

        self.last_beacon_finality_head_checked = next_head;
    }

    /// Event loop

    pub async fn run(mut self) {
        // During initial startup we need to immediately check if genesis finality head has moved in order to apply any updates
        // that happened while this process was offline

        let _ = self
            .trigger_listener_with_notice(NoticeMessageExtension::Started(NoticeStarted {}))
            .await;

        // This could be useful in the future be lets be careful. If there is nothing to invoke advance the it would not auto emit
        /*let offline_finality_update_next_head = self.check_finality_next_head().await.unwrap(); // Panic if the rpc is down.
        if self.current_head < offline_finality_update_next_head || self.bootstrap {
            // Immediately spawn a sp1-prover job to update the bridge based on the checkpoint head value
            self.create_prover_job(self.next_head, self.job_idx);
        }*/
        info!("Event loop started. Launching a proof job defensively.");
        let _ = self
            .create_prover_job(self.job_idx, self.next_head, self.next_sync_committee)
            .await;

        let mut command_rx = self.command_rx.take().unwrap();
        let mut job_rx = self.job_rx.take().unwrap();

        loop {
            tokio::select! {
                // Read the command receiver for input commands
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        EventLoopCommand::Advance => {
                            // deal with advance invocation
                            self.advance().await;
                        }
                        EventLoopCommand::BeaconFinalityChange(message) => {
                            let next_slot = message.slot;
                            self.on_beacon_finality_change(next_slot).await;
                        }
                    }
                }
                // Read the job receiver for returned jobs
                Some(job_result) = job_rx.recv() => {
                    match job_result {
                        Ok(result_data) => {
                            let job: &ProverJob = self.prover_jobs.get(&result_data.job_idx()).unwrap();
                            let time_taken_second = Instant::now()
                                .duration_since(job.start_instant)
                                .as_secs_f64();

                            info!(
                                "Job '{}' finished in {} seconds.",
                                result_data.job_idx(), time_taken_second
                            );

                            let _ = self.handle_prover_success(
                                result_data.slot(),
                                result_data.proof(),
                                result_data.job_idx(),
                                time_taken_second,
                            ).await;

                            self.last_job_duration_sec = time_taken_second;
                        }
                        Err(err) => {
                            self.handle_prover_failure(&err).await;
                        }
                    }
                }
            }
        }
    }
}
