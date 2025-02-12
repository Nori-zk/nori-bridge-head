use crate::sp1_prover::finality_update_job;
use crate::{event_dispatcher::EventListener, proof_outputs_decoder::DecodedProofOutputs};
use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_consensus_core::types::FinalityUpdate;
use helios_ethereum::rpc::ConsensusRpc;
use helios_ethereum::{consensus::Inner, rpc::http_rpc::HttpRpc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use sp1_helios_script::{get_checkpoint, get_client, get_latest_checkpoint};
use sp1_sdk::SP1ProofWithPublicValues;
use std::{collections::HashMap, default, env, fs::File, io::Read, path::Path, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use tree_hash::TreeHash;

const NB_CHECKPOINT_FILE: &str = "nb_checkpoint.json";

#[derive(Serialize, Deserialize)]
pub struct NoriBridgeCheckpoint {
    slot_head: u64,
    next_sync_committee: FixedBytes<32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoriBridgeHeadNoticeMessage {}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoriBridgeHeadProofMessage {
    pub slot: u64,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
}

struct ProverJobArgument {
    slot: u64,
    last_next_sync_committee: FixedBytes<32>,
} // TX

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
    rx: mpsc::Receiver<ProverJobOutput>,
}


pub enum NoriBridgeEventLoopCommand {
    Advance,
    AddProofListener {
        listener: Arc<Mutex<dyn EventListener<NoriBridgeHeadProofMessage>>>,
    },
}
pub struct NoriBridgeHeadEventLoopConfig {}

/*impl NoriBridgeHeadEventLoopConfig {
    pub fn new() -> Self {
        Self {}
    }
}*/

pub struct BridgeHeadEventLoop { // <'a>
    polling_interval_sec: f64,

    current_head: u64,
    next_head: u64,
    working_head: u64,
    auto_advance_index: u64,
    job_idx: u64,
    next_sync_committee: FixedBytes<32>,

    prover_jobs: HashMap<u64, ProverJob>,

    //current_sync_commitee: FixedBytes<32>,
    helios_polling_client: Inner<MainnetConsensusSpec, HttpRpc>,
    //bridge_mode: NoriBridgeHeadMode,

    //notice_dispatcher: EventDispatcher<NoriBridgeHeadNoticeMessage>,
    //proof_dispatcher: EventDispatcher<NoriBridgeHeadProofMessage>,
    proof_listeners: Vec<Arc<Mutex<dyn EventListener<NoriBridgeHeadProofMessage>  + Send + Sync>>>, // notice_dispatcher

    command_receiver: mpsc::Receiver<NoriBridgeEventLoopCommand>,
}

impl BridgeHeadEventLoop { // <'a: 'static> <'a>
    // new
    pub async fn new(command_receiver: mpsc::Receiver<NoriBridgeEventLoopCommand>) -> Self {
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
        if BridgeHeadEventLoop::nb_checkpoint_exists(NB_CHECKPOINT_FILE) {
            // Warm start procedure
            info!("Loading nori slot checkpoint from file.");
            let nb_checkpoint =
                BridgeHeadEventLoop::load_nb_checkpoint(NB_CHECKPOINT_FILE).unwrap();
            current_head = nb_checkpoint.slot_head;
            next_sync_committee = nb_checkpoint.next_sync_committee;
        } else {
            // Cold start procedure
            info!("Resorting to cold start procedure.");
            current_head = BridgeHeadEventLoop::get_cold_finality_current_head().await;
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

            auto_advance_index: 1,
            job_idx: 1,

            next_sync_committee,

            helios_polling_client,

            proof_listeners: vec![],
            prover_jobs: HashMap::new(),

            command_receiver,
        }
    }

    async fn get_cold_finality_current_head() -> u64 {
        // Get latest beacon checkpoint
        let helios_checkpoint = get_latest_checkpoint().await;

        // Get the client from the beacon checkpoint
        let helios_client = get_client(helios_checkpoint).await;

        // Get slot head from checkpoint
        helios_client.store.finalized_header.clone().beacon().slot
    }

    // create prover job
    fn create_prover_job(&mut self, slot: u64, job_idx: u64) {
        self.working_head = slot; // Mark the working head as what was given by the slot

        info!(
            "Nori head updater recieved a new job {}. Spawning a new worker.",
            job_idx
        );

        let last_next_sync_committee = self.next_sync_committee;
        let slot_clone = slot;

        let (tx, rx) = mpsc::channel(1);
        self.prover_jobs.insert(job_idx, ProverJob { rx });

        // Spawn proof job in worker thread (check for blocking)
        tokio::spawn(async move {
            let proof = finality_update_job(slot_clone, last_next_sync_committee)
                .await
                .unwrap();
            let prover_job_output = ProverJobOutput {
                slot: slot_clone,
                proof,
            };
            tx.send(prover_job_output).await.unwrap();
        });
    }

    // handle prover job output
    async fn handle_prover_output(
        &mut self,
        slot: u64,
        proof: SP1ProofWithPublicValues,
        job_idx: u64,
    ) -> Result<()> {
        // Check if our result is still relevant after the computation, if our working_head has advanced then we are working on a more recent head in another thread.
        if self.working_head == slot {
            // could move this to a job_id check vs auto_advance_index if we really wanted to skip what we've got in place with advance being called.
            // Update our state
            info!("Moving nori head forward.");

            // Extract the next sync committee out of the proof output
            let public_values: sp1_sdk::SP1PublicValues = proof.clone().public_values;
            let public_values_bytes = public_values.as_slice(); // Raw bytes
            let proof_outputs = DecodedProofOutputs::from_abi(public_values_bytes)?;

            // Update head and next_sync_committee based on the proof outputs
            self.current_head = slot;
            if proof_outputs.next_sync_committee_hash != FixedBytes::<32>::default() {
                self.next_sync_committee = proof_outputs.next_sync_committee_hash;
                // But wait! We need to check the logic in SP1Helios.sol as this can be Zeros
            }
            // Save the checkpoint
            self.save_nb_checkpoint();

            /*
               Need to carefully deactivate auto_advance, but lets think about this....
               We might have had advance called multiple times we need some concept of the job number to know if we should prevent auto advancement.
               If this task was the last auto advance task we should cancel that behaviour
            */
            if job_idx >= self.auto_advance_index {
                // gt to account for if a previous job spawned failed with an error and didnt cancel itself
                info!(
                    "Cancelling auto advance for job {}",
                    self.auto_advance_index
                );
                self.auto_advance_index = 0;
            }

            self.trigger_proof_listeners(NoriBridgeHeadProofMessage {
                slot,
                proof,
                execution_state_root: proof_outputs.execution_state_root,
            })
            .await?;
        }
        Ok(())
    }

    // I heart the CCP
    async fn trigger_proof_listeners(&mut self, payload: NoriBridgeHeadProofMessage) -> Result<()> {
        use futures::future::join_all;

        let futures = self
            .proof_listeners
            .iter()
            .map(|listener| {
                let listener = listener.clone();
                let payload_clone = payload.clone();
                async move {
                    let mut listener_lock = listener.lock().await;
                    listener_lock.on_event(payload_clone).await
                }
            })
            .collect::<Vec<_>>();

        let results = join_all(futures).await;
        results.into_iter().collect::<Result<()>>()?;

        Ok(())
    }

    // handler prover jobs
    async fn check_prover_jobs(&mut self) -> Result<()> {
        let mut completed = Vec::new();
        let mut results: Vec<ProverJobOutputWithJob> = Vec::new();

        // Extract jobs
        for (&job_idx, job) in self.prover_jobs.iter_mut() {
            if let Ok(result) = job.rx.try_recv() {
                // println!("{}", result);
                results.push(ProverJobOutputWithJob {
                    job_idx,
                    proof: result.proof,
                    slot: result.slot,
                });
                completed.push(job_idx);
            }
        }

        // Process completed output
        for result in results.iter_mut() {
            self.handle_prover_output(result.slot, result.proof.clone(), result.job_idx)
                .await?;
        }

        // Cleanup hash map
        for job_idx in completed {
            self.prover_jobs.remove(&job_idx);
        }

        Ok(())
    }

    // advance
    async fn advance(&mut self) {
        self.job_idx += 1;
        info!("Advance called ready for job {}", self.job_idx);
        if self.next_head > self.current_head {
            info!("Immediately trying to advance");
            // We should immediately try to create a new proof
            self.create_prover_job(self.next_head, self.job_idx);
        } else {
            info!("Setting flag to attempt auto advance head on next finality update.");
            // We should wait for the next detected head update.
            self.auto_advance_index = self.job_idx;
        }
    }

    async fn check_finality_next_head(&self) -> Result<u64> {
        // Get finality slot head
        info!("Checking finality slot head.");
        let finality_update: FinalityUpdate<MainnetConsensusSpec> = self
            .helios_polling_client
            .rpc
            .get_finality_update()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

        // Extract latest slot
        Ok(finality_update.finalized_header.beacon().slot)
    }

    // loop
    pub async fn run_loop(mut self) {
        // Check helios for the next finality head

        // check our current head vs the next head
        // --> Do nothing if the head is up to date (listening for commands and listening for job updates)
        loop {
            // Read the command_receiver which gets messages from the parent thread

            // Try to receive a new command without blocking
            match self.command_receiver.try_recv() {
                Ok(cmd) => match cmd {
                    NoriBridgeEventLoopCommand::Advance => {
                        // deal with advance invocation
                        self.advance().await;
                    }
                    NoriBridgeEventLoopCommand::AddProofListener { listener } => {
                        // Do something with this listener
                        self.proof_listeners.push(listener); // Arc<Mutex<dyn EventListener<NoriBridgeHeadProofMessage>>>
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
            match self.check_finality_next_head().await {
                Ok(next_head) => {
                    // Have the helios finality head moved forwards
                    if next_head <= self.current_head {
                        info!("Nori bridge is up to date.");
                    } else {
                        info!("Nori bridge is stale. Setting next head");
                        self.next_head = next_head;
                        if self.auto_advance_index != 0 {
                            // Invoke a job
                            self.create_prover_job(self.next_head, self.job_idx);
                        }
                    }
                }
                Err(e) => {
                    error!("Error checking finality slot head: {}", e);
                }
            }

            // Check the status of the prover jobs
            if let Err(e) = self.check_prover_jobs().await {
                error!("Prover job check failed: {}", e);
            }

            // Think about sleeping because we dont want to hammer the RPC (TODO later...maybe)

            // Yield to other tasks instead of sleeping
            tokio::task::yield_now().await;
        }
    }

    // Static method to check if the checkpoint file exists
    fn nb_checkpoint_exists(nb_checkpoint_location: &str) -> bool {
        Path::new(nb_checkpoint_location).exists()
    }

    // Static method to load the checkpoint from file
    fn load_nb_checkpoint(nb_checkpoint_location: &str) -> Result<NoriBridgeCheckpoint> {
        // Open the checkpoint file
        let mut file =
            File::open(nb_checkpoint_location).expect("Failed to open nori checkpoint file.");

        // Read the contents into a Vec<u8>
        let mut serialized_checkpoint = Vec::new();
        file.read_to_end(&mut serialized_checkpoint)
            .expect("Failed to read nori checkpoint file.");

        // Deserialize the checkpoint data using serde_json (not serde_cbor)
        let nb_checkpoint: NoriBridgeCheckpoint = serde_json::from_slice(&serialized_checkpoint)
            .expect("Failed to deserialize nori checkpoint.");

        Ok(nb_checkpoint)
    }

    fn save_nb_checkpoint(&self) {
        // Define the current checkpoint
        let checkpoint = NoriBridgeCheckpoint {
            slot_head: self.current_head,
            next_sync_committee: self.next_sync_committee,
        };

        // Serialize the checkpoint to a byte vector
        let serialized_nb_checkpoint =
            serde_json::to_string(&checkpoint).expect("Failed to serialize nori checkpoint.");

        // Write the serialized data to the file specified by `checkpoint_location`
        std::fs::write(NB_CHECKPOINT_FILE, &serialized_nb_checkpoint)
            .map_err(|e| anyhow::anyhow!("Failed to write to checkpoint file: {}", e))
            .unwrap();

        info!("Nori bridge checkpoint saved successfully.");
    }
}