use super::notice_messages::{
    get_nori_notice_message_type, NoriBridgeHeadMessageExtension,
    NoriBridgeHeadNoticeAdvanceRequested, NoriBridgeHeadNoticeBaseMessage,
    NoriBridgeHeadNoticeFinalityTransitionDetected, NoriBridgeHeadNoticeHeadAdvanced,
    NoriBridgeHeadNoticeJobCreated, NoriBridgeHeadNoticeJobFailed,
    NoriBridgeHeadNoticeJobSucceeded, NoriBridgeHeadNoticeMessage, NoriBridgeHeadNoticeStarted,
};
use crate::bridge_head::checkpoint::{load_nb_checkpoint, nb_checkpoint_exists, save_nb_checkpoint};
use crate::helios::{get_client_latest_finality_head, get_latest_finality_head};
use crate::sp1_prover::finality_update_job;
use crate::proof_outputs_decoder::DecodedProofOutputs;
use alloy_primitives::FixedBytes;
use anyhow::{Error, Result};
use chrono::{SecondsFormat, Utc};
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_consensus_core::types::FinalityUpdate;
use helios_ethereum::rpc::ConsensusRpc;
use helios_ethereum::{consensus::Inner, rpc::http_rpc::HttpRpc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use sp1_helios_script::{get_checkpoint, get_client, get_latest_checkpoint};
use sp1_sdk::SP1ProofWithPublicValues;
use std::{collections::HashMap, env, fs::File, io::Read, path::Path};
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::event_handler::NoriBridgeHeadEventProducer;

const TYPICAL_FINALITY_TRANSITION_TIME: f64 = 384.0;

/// Proof types
#[derive(Serialize, Deserialize, Clone)]
pub struct NoriBridgeHeadProofMessage {
    pub slot: u64,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
    pub next_sync_committee: FixedBytes<32>
}
struct ProverJobOutput {
    proof: SP1ProofWithPublicValues,
    slot: u64,
}

struct ProverJobOutputWithJob {
    proof: SP1ProofWithPublicValues,
    slot: u64,
    job_idx: u64,
}

struct ProverJob {
    rx: mpsc::Receiver<Result<ProverJobOutput, Error>>,
    start_instant: Instant,
    slot: u64,
    next_sync_committee: FixedBytes<32>,
}

/// Event loop commands
pub enum NoriBridgeEventLoopCommand {
    Advance,
}

/// Bridge Head
pub struct BridgeHeadEventLoop {
    polling_interval_sec: f64,
    current_head: u64,
    next_head: u64,
    working_head: u64,
    last_beacon_finality_head_checked: u64,
    auto_advance_index: u64,
    job_idx: u64,
    last_job_duration_sec: f64,
    last_finality_transition_instant: Instant,
    next_sync_committee: FixedBytes<32>,
    prover_jobs: HashMap<u64, ProverJob>,
    helios_polling_client: Inner<MainnetConsensusSpec, HttpRpc>,
    event_listener: Box<
        dyn NoriBridgeHeadEventProducer<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage>
            + Send
            + Sync,
    >,
    command_receiver: mpsc::Receiver<NoriBridgeEventLoopCommand>,
}

impl BridgeHeadEventLoop {
    pub async fn new(
        command_receiver: mpsc::Receiver<NoriBridgeEventLoopCommand>,
        listener: Box<
            dyn NoriBridgeHeadEventProducer<NoriBridgeHeadProofMessage, NoriBridgeHeadNoticeMessage>
                + Send
                + Sync,
        >,
    ) -> Self {
        dotenv::dotenv().ok();

        // Define sleep interval
        let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
            .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
            .parse()
            .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

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

        // Startup info
        info!("Starting nori bridge.");
        info!("Starting beacon client.");

        // Get beacon checkpoint
        let helios_checkpoint = get_checkpoint(current_head).await;

        // Get the client from the beacon checkpoint
        let helios_polling_client = get_client(helios_checkpoint).await;

        Self {
            polling_interval_sec,
            current_head,
            next_head: current_head,
            working_head: current_head,
            last_beacon_finality_head_checked: u64::default(),
            auto_advance_index: 1,
            job_idx: 1,
            last_job_duration_sec: 0.0,
            last_finality_transition_instant: Instant::now(),
            next_sync_committee,
            helios_polling_client,
            event_listener: listener,
            prover_jobs: HashMap::new(),
            command_receiver,
        }
    }

    /// Event dispatchers

    //  Emit proofs
    async fn trigger_listener_with_proof(
        &mut self,
        payload: NoriBridgeHeadProofMessage,
    ) -> Result<()> {
        let _ = self.event_listener.as_mut().on_proof(payload).await;
        Ok(())
    }

    // Emit notices
    async fn trigger_listener_with_notice(
        &mut self,
        extension: NoriBridgeHeadMessageExtension,
    ) -> Result<()> {
        let now = Utc::now();
        let iso_string = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let message_type = get_nori_notice_message_type(&extension);
        let base_message = NoriBridgeHeadNoticeBaseMessage {
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
        let full_message = NoriBridgeHeadNoticeMessage {
            base: base_message,
            extension,
        };
        let _ = self.event_listener.as_mut().on_notice(full_message).await;
        Ok(())
    }

    /// Jobs

    // Create prover job
    async fn create_prover_job(
        &mut self,
        slot: u64,
        job_idx: u64,
        next_sync_committee: FixedBytes<32>,
    ) {
        info!(
            "Nori head updater recieved a new job {}. Spawning a new worker.",
            job_idx
        );

        self.working_head = slot; // Mark the working head as what was given by the slot

        //let last_next_sync_committee = self.next_sync_committee;
        let slot_clone = slot;

        let (tx, rx) = mpsc::channel(1);
        self.prover_jobs.insert(
            job_idx,
            ProverJob {
                rx,
                start_instant: Instant::now(),
                next_sync_committee,
                slot,
            },
        );

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            let proof_result = finality_update_job(slot_clone, next_sync_committee).await;

            match proof_result {
                Ok(proof) => {
                    let prover_job_output = ProverJobOutput {
                        slot: slot_clone,
                        proof,
                    };
                    tx.send(Ok(prover_job_output)).await.unwrap();
                }
                Err(err) => {
                    tx.send(Err(err)).await.unwrap();
                }
            }
        });

        /*
           Need to carefully deactivate auto_advance, but lets think about this....
           We might have had advance called multiple times we need some concept of the job number to know if we should prevent auto advancement.
           If this task was the last auto advance task we should cancel that behaviour
        */
        if job_idx >= self.auto_advance_index {
            // Do we need the if... this should always be true CHECKME
            // gt to account for if a previous job spawned failed with an error and didnt cancel itself
            info!(
                "Cancelling auto advance for job '{}'.",
                self.auto_advance_index
            );
            self.auto_advance_index = 0;
        }

        let _ = self
            .trigger_listener_with_notice(
                NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobCreated(
                    NoriBridgeHeadNoticeJobCreated { slot, job_idx },
                ),
            )
            .await;
    }

    // Handle prover job output
    async fn handle_prover_output(
        &mut self,
        slot: u64,
        proof: SP1ProofWithPublicValues,
        job_idx: u64,
    ) -> Result<()> {
        info!("Handling prover job output '{}'.", job_idx);

        // Extract the next sync committee out of the proof output
        let public_values: sp1_sdk::SP1PublicValues = proof.clone().public_values;
        let public_values_bytes = public_values.as_slice(); // Raw bytes
        let proof_outputs = DecodedProofOutputs::from_abi(public_values_bytes)?;

        // Notify of a succesful job
        let _ = self
            .trigger_listener_with_notice(
                NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobSucceeded(
                    NoriBridgeHeadNoticeJobSucceeded {
                        slot,
                        job_idx,
                        next_sync_committee: self.next_sync_committee,
                    },
                ),
            )
            .await;

        // Check if our result is still relevant after the computation, if our working_head has advanced then we are working on a more recent head in another thread.
        if self.working_head == slot {
            // could move this to a job_id check vs auto_advance_index if we really wanted to skip what we've got in place with advance being called.
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
                .trigger_listener_with_proof(NoriBridgeHeadProofMessage {
                    slot,
                    proof,
                    execution_state_root: proof_outputs.execution_state_root,
                    next_sync_committee: proof_outputs.next_sync_committee_hash
                })
                .await;

            // Notify of head advance
            let _ = self
                .trigger_listener_with_notice(
                    NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeHeadAdvanced(
                        NoriBridgeHeadNoticeHeadAdvanced {
                            slot,
                            next_sync_committee: self.next_sync_committee,
                        },
                    ),
                )
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

    // Handler prover jobs
    async fn check_prover_jobs(&mut self) -> Result<()> {
        let mut completed = Vec::new();
        let mut results: Vec<ProverJobOutputWithJob> = Vec::new();
        let mut failed: Vec<(u64, String)> = Vec::new();

        // Extract jobs
        for (&job_idx, job) in self.prover_jobs.iter_mut() {
            if let Ok(result) = job.rx.try_recv() {
                match result {
                    Ok(result_data) => {
                        self.last_job_duration_sec = Instant::now()
                            .duration_since(job.start_instant)
                            .as_secs_f64();
                        info!(
                            "Job '{}' finished in {} seconds.",
                            job_idx, self.last_job_duration_sec
                        );
                        results.push(ProverJobOutputWithJob {
                            job_idx,
                            proof: result_data.proof,
                            slot: result_data.slot,
                        });
                        completed.push(job_idx);
                    }
                    Err(err) => {
                        // Handle the error case if the inner Result is Err
                        let message = format!("Job '{}' failed with error: {}", job_idx, err);
                        error!("Job '{}' failed with error: {}", job_idx, err);
                        failed.push((job_idx, message));
                    }
                }
            }
        }

        // Process completed output
        for result in results.iter_mut() {
            self.handle_prover_output(result.slot, result.proof.clone(), result.job_idx)
                .await?;
        }

        // Process failed jobs
        for failed_job in failed.clone() {
            // If our current job failed restart it...
            if self.job_idx == failed_job.0 {
                let job: &ProverJob = self.prover_jobs.get(&failed_job.0).unwrap();
                let _ = self
                    .create_prover_job(job.slot, failed_job.0, job.next_sync_committee)
                    .await;
            }
        }

        // Emit job failure notices
        for failed_job in failed.clone() {
            let job: &ProverJob = self.prover_jobs.get(&failed_job.0).unwrap();
            let _ = self
                .trigger_listener_with_notice(
                    NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeJobFailed(
                        NoriBridgeHeadNoticeJobFailed {
                            slot: job.slot,
                            job_idx: failed_job.0,
                            message: failed_job.1,
                        },
                    ),
                )
                .await;
        }

        // Cleanup hash map
        for job_idx in completed {
            self.prover_jobs.remove(&job_idx);
        }

        Ok(())
    }

    /// Commands

    // advance
    async fn advance(&mut self) {
        self.job_idx += 1;
        info!("Advance called ready for job '{}'.", self.job_idx);
        if self.next_head > self.current_head {
            // TODO think about the time until the next transition
            if self.last_job_duration_sec != 0.0 {
                // If we have data on the last job time

                let seconds_since_last_transition = Instant::now()
                    .duration_since(self.last_finality_transition_instant)
                    .as_secs_f64();

                if self.last_job_duration_sec > TYPICAL_FINALITY_TRANSITION_TIME {
                    warn!("Long last job duration '{}' seconds, compared to the typical finality transition time '{}'. If this continues nori bridge will definitely not be able to catch up.", self.last_job_duration_sec, TYPICAL_FINALITY_TRANSITION_TIME);
                } else if self.last_job_duration_sec > TYPICAL_FINALITY_TRANSITION_TIME * 0.8 {
                    warn!("Long last job duration '{}' seconds, compared to the typical finality transition time '{}'. If this continues nori bridge will take a while to catch up.", self.last_job_duration_sec, TYPICAL_FINALITY_TRANSITION_TIME);
                }

                info!(
                    "Expected time to next finality transition is {} seconds.",
                    TYPICAL_FINALITY_TRANSITION_TIME - seconds_since_last_transition
                );

                if seconds_since_last_transition > 0.5 * TYPICAL_FINALITY_TRANSITION_TIME {
                    warn!("We are within 50% of the typical finality transition time away from the next finality transition. Strategically waiting for the next finality transition in order to try to catch up more quickly. Note this will cause latency for current transactions in the bridge.");
                    // We should wait for the next detected head update.
                    self.auto_advance_index = self.job_idx;
                }
            }

            // We should immediately try to create a new proof
            if self.auto_advance_index != self.job_idx {
                info!("Immediately trying to advance.");
                let _ = self
                    .create_prover_job(self.next_head, self.job_idx, self.next_sync_committee)
                    .await;
            }
        } else {
            info!("Setting flag to attempt auto advance head on next finality update, as nori bridge is currently up to date.");
            // We should wait for the next detected head update.
            self.auto_advance_index = self.job_idx;
        }
        let _ = self
            .trigger_listener_with_notice(
                NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeAdvanceRequested(
                    NoriBridgeHeadNoticeAdvanceRequested {},
                ),
            )
            .await;
    }

    /// Event loop

    pub async fn run_loop(mut self) {
        // During initial startup we need to immediately check if genesis finality head has moved in order to apply any updates
        // that happened while this process was offline

        let _ = self
            .trigger_listener_with_notice(
                NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeStarted(
                    NoriBridgeHeadNoticeStarted {},
                ),
            )
            .await;

        // This could be useful in the future be lets be careful. If there is nothing to invoke advance the it would not auto emit
        /*let offline_finality_update_next_head = self.check_finality_next_head().await.unwrap(); // Panic if the rpc is down.
        if self.current_head < offline_finality_update_next_head || self.bootstrap {
            // Immediately spawn a sp1-prover job to update the bridge based on the checkpoint head value
            self.create_prover_job(self.next_head, self.job_idx);
        }*/
        info!("Event loop started. Launching a proof job defensively.");
        let _ = self
            .create_prover_job(self.next_head, self.job_idx, self.next_sync_committee)
            .await;

        // Store last execution time before main loop
        let mut last_check_time = Instant::now();

        loop {
            // Read the command_receiver which gets messages from the parent thread
            match self.command_receiver.try_recv() {
                Ok(cmd) => match cmd {
                    NoriBridgeEventLoopCommand::Advance => {
                        // deal with advance invocation
                        self.advance().await;
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No new commands, that's fine
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Channel closed, but we'll keep running to finish existing jobs
                }
            }

            // Check state of the head
            let now = Instant::now(); // Capture the current time at the start of the loop
            if now.duration_since(last_check_time).as_secs_f64() >= self.polling_interval_sec {
                info!("Polling helios finality slot head.");
                match get_client_latest_finality_head(&self.helios_polling_client).await {
                    Ok(next_head) => {
                        // Only run if the RPC didnt give us an old value
                        if next_head >= self.last_beacon_finality_head_checked {
                            // Have the helios finality head moved forwards
                            if next_head <= self.current_head {
                                if self.last_beacon_finality_head_checked != next_head {
                                    info!(
                                        "Nori bridge head is up to date. Current head is '{}'.",
                                        self.current_head
                                    );
                                }
                            } else {
                                // Next head is greater than the (current head and the last_beacon_finality_head_checked)
                                if self.last_beacon_finality_head_checked != next_head {
                                    // We have a transition!
                                    // Update next head
                                    self.next_head = next_head;
                                    // Print the head change detection
                                    self.last_finality_transition_instant = Instant::now();
                                    info!("Helios beacon finality slot change detected. Nori bridge head is stale. Current head is: '{}' Working head is '{}' Beacon finality head (next_head) is: '{}', Updating next_head.", self.current_head, self.working_head, self.next_head);
                                    // Auto advance if nessesary
                                    if self.auto_advance_index != 0 {
                                        info!(
                                            "Auto advance invoking job due to finality transition."
                                        );
                                        // Invoke a job
                                        let _ = self
                                            .create_prover_job(
                                                self.next_head,
                                                self.job_idx,
                                                self.next_sync_committee,
                                            )
                                            .await;
                                    }
                                    // Notify of transition
                                    let _ = self
                                    .trigger_listener_with_notice(
                                        NoriBridgeHeadMessageExtension::NoriBridgeHeadNoticeFinalityTransitionDetected(
                                            NoriBridgeHeadNoticeFinalityTransitionDetected {
                                                slot: next_head
                                            },
                                        ),
                                    )
                                    .await;
                                }
                            }

                            // Update the last head we have checked
                            self.last_beacon_finality_head_checked = next_head;
                        }
                    }
                    Err(e) => {
                        error!("Error checking finality slot head: {}", e);
                    }
                }
                last_check_time = now; // Update last check time after performing the check
            }

            // Check the status of the prover jobs
            if let Err(e) = self.check_prover_jobs().await {
                error!("Prover job check failed: {}", e);
            }

            // Yield to other tasks instead of sleeping
            tokio::task::yield_now().await;
        }
    }
}
