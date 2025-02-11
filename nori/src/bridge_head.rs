use crate::{
    event_dispatcher::{EventDispatcher, EventListener},
    proof_outputs_decoder::DecodedProofOutputs,
    utils::{get_finality_updates, handle_nori_proof},
};
use alloy_primitives::{FixedBytes, B256};
use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec, types::FinalityUpdate};
use helios_ethereum::consensus::Inner;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use helios_ethereum::rpc::ConsensusRpc;
use log::{error, info};
use serde::{Deserialize, Serialize};
use sp1_helios_primitives::types::ProofInputs;
use sp1_helios_script::{get_checkpoint, get_client, get_latest_checkpoint};
use sp1_sdk::{EnvProver, ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use std::{env, fmt, fs::File, io::Read, path::Path, str::FromStr};
use tokio::runtime::Handle;
use tokio::task;
use tokio::time::{sleep, Duration};
use tree_hash::TreeHash;

const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf");
const NB_CHECKPOINT_FILE: &str = "nb_checkpoint.json";

#[derive(PartialEq)]
pub enum NoriBridgeHeadMode {
    Finality = 1,
    Optimistic = 2,
}

impl FromStr for NoriBridgeHeadMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "finality" => Ok(NoriBridgeHeadMode::Finality),
            "optimistic" => Ok(NoriBridgeHeadMode::Optimistic),
            _ => Err(format!(
                "Invalid value for NoriBridgeHeadMode: '{}'. Expected 'finality' or 'optimistic'.",
                s
            )),
        }
    }
}

impl fmt::Display for NoriBridgeHeadMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NoriBridgeHeadMode::Finality => write!(f, "Finality"),
            NoriBridgeHeadMode::Optimistic => write!(f, "Optimistic"),
        }
    }
}

pub struct NoriBridgeHeadConfig {
    bridge_mode: NoriBridgeHeadMode,
}

impl NoriBridgeHeadConfig {
    pub fn new(bridge_mode: NoriBridgeHeadMode) -> Self {
        Self { bridge_mode }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoriBridgeCheckpoint {
    slot_head: u64,
    current_sync_commitee: FixedBytes<32>,
    next_sync_committee: FixedBytes<32>,
}

pub struct NoriBridgeHead {
    polling_interval_sec: f64,
    current_head: u64,
    next_head: u64,
    working_head: u64,
    next_sync_committee: FixedBytes<32>,
    current_sync_commitee: FixedBytes<32>,
    helios_polling_client: Inner<MainnetConsensusSpec, HttpRpc>,
    bridge_mode: NoriBridgeHeadMode,
    auto_advance_index: u64,
    notice_dispatcher: EventDispatcher<NoriBridgeHeadNoticeMessage>,
    proof_dispatcher: EventDispatcher<NoriBridgeHeadProofMessage>,
    job_idx: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoriBridgeHeadNoticeMessage {}
#[derive(Serialize, Deserialize, Clone)]
pub struct NoriBridgeHeadProofMessage {
    pub slot: u64,
    pub proof: SP1ProofWithPublicValues,
    pub execution_state_root: FixedBytes<32>,
}

async fn finality_update_job(
    slot: u64,
    last_next_sync_committee: FixedBytes<32>,
) -> Result<SP1ProofWithPublicValues> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(slot).await; // This panics FIXME

    // Re init helios client
    let mut heliod_update_client = get_client(helios_checkpoint).await; // This panics FIXME

    // Get finality update
    info!("Getting finality update.");
    let finality_update = heliod_update_client
        .rpc
        .get_finality_update()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

    // Get sync commitee updates
    info!("Getting sync commitee updates.");
    let mut sync_committee_updates = get_finality_updates(&heliod_update_client).await; // This panics FIXME

    // Taken from operator.rs
    // Optimization:
    // Skip processing update inside program if next_sync_committee is already stored in contract.
    // We must still apply the update locally to "sync" the helios client, this is due to
    // next_sync_committee not being stored when the helios client is bootstrapped.
    info!("Applying sync committee optimisation.");
    if !sync_committee_updates.is_empty() {
        let next_sync_committee = B256::from_slice(
            sync_committee_updates[0]
                .next_sync_committee
                .tree_hash_root()
                .as_ref(),
        );

        info!("Comparing sync comitee info");
        info!("last_next_sync_committee {}", last_next_sync_committee);
        info!("next_sync_committee {}", next_sync_committee);
        if last_next_sync_committee == next_sync_committee {
            // self.next_sync_committee
            info!("Applying optimization, skipping sync committee update.");
            let temp_update = sync_committee_updates.remove(0);

            heliod_update_client.verify_update(&temp_update).unwrap(); // Panics if not valid FIXME?
            heliod_update_client.apply_update(&temp_update);
        }
    }

    // Create program inputs
    info!("Building sp1 proof inputs.");
    let mut stdin = SP1Stdin::new();
    let expected_current_slot = heliod_update_client.expected_current_slot();
    let inputs = ProofInputs {
        sync_committee_updates,
        finality_update,
        expected_current_slot,
        store: heliod_update_client.store.clone(),
        genesis_root: heliod_update_client.config.chain.genesis_root,
        forks: heliod_update_client.config.forks.clone(),
    };

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
    stdin.write_slice(&encoded_proof_inputs);

    // Generate proof.
    info!("Running sp1 proof.");
    // Get prover client and pk
    let prover_client = ProverClient::from_env();
    let (pk, _) = prover_client.setup(ELF);
    let proof = prover_client.prove(&pk, &stdin).plonk().run()?;

    Ok(proof)
}

impl NoriBridgeHead {
    pub async fn new(config: NoriBridgeHeadConfig) -> Self {
        dotenv::dotenv().ok();

        // Check bridge mode configuration
        if config.bridge_mode == NoriBridgeHeadMode::Optimistic {
            panic!("Nori bridge mode Optimistic is not currently supported.");
        }

        // Define sleep interval
        let polling_interval_sec: f64 = env::var("NORI_HELIOS_POLLING_INTERVAL")
            .unwrap_or_else(|_| "1.0".to_string()) // Default to 1.0 if not set
            .parse()
            .expect("Failed to parse NORI_HELIOS_POLLING_INTERVAL as f64.");

        // Initialise slot head / commitee vars
        let current_head;
        let mut next_sync_committee = FixedBytes::<32>::default();
        let mut current_sync_commitee = FixedBytes::<32>::default();
        let mut cold_start = false;

        // Start procedure
        if NoriBridgeHead::nb_checkpoint_exists(NB_CHECKPOINT_FILE) {
            // Warm start procedure
            info!("Loading nori slot checkpoint from file.");
            let nb_checkpoint = NoriBridgeHead::load_nb_checkpoint(NB_CHECKPOINT_FILE).unwrap();
            current_head = nb_checkpoint.slot_head;
            next_sync_committee = nb_checkpoint.next_sync_committee;
            current_sync_commitee = nb_checkpoint.current_sync_commitee;
        } else {
            // Cold start procedure
            info!("Resorting to cold start procedure.");
            current_head = NoriBridgeHead::get_cold_finality_current_head().await;
            cold_start = true;
        }

        // Startup info
        info!("Starting nori bridge in '{}' mode.", config.bridge_mode);
        info!("Starting beacon client.");

        // Get beacon checkpoint
        let helios_checkpoint = get_checkpoint(current_head).await;

        // Get the client from the beacon checkpoint
        let helios_polling_client = get_client(helios_checkpoint).await;

        // Get sync commitee if we are cold starting (see genesis)
        if cold_start {
            current_sync_commitee = helios_polling_client
                .store
                .current_sync_committee
                .clone()
                .tree_hash_root();
        }

        Self {
            polling_interval_sec,
            current_head,
            next_head: current_head,
            working_head: current_head,
            next_sync_committee,
            current_sync_commitee,
            helios_polling_client,
            bridge_mode: config.bridge_mode,
            auto_advance_index: 1,
            notice_dispatcher: EventDispatcher::<NoriBridgeHeadNoticeMessage>::new(),
            proof_dispatcher: EventDispatcher::<NoriBridgeHeadProofMessage>::new(),
            job_idx: 1,
        }
    }

    /*pub fn add_proof_listener<L>(&mut self, listener: L) 
    where 
        L: EventListener<NoriBridgeHeadProofMessage> + 'static + Send, 
    {
        self.proof_dispatcher.add_listener(listener);
    }*/

    /*pub fn add_proof_listener<L>(&mut self, listener: L) 
    where
        L: EventListener<NoriBridgeHeadProofMessage> + 'static + Send,
    {
        self.proof_dispatcher.add_listener(listener); // This will handle boxing internally
    }*/
    pub fn add_proof_listener(&mut self, listener: Box<dyn EventListener<NoriBridgeHeadProofMessage> + Send>) {
        self.proof_dispatcher.add_listener(listener);
    }

    async fn get_cold_finality_current_head() -> u64 {
        // Get latest beacon checkpoint
        let helios_checkpoint = get_latest_checkpoint().await;

        // Get the client from the beacon checkpoint
        let helios_client = get_client(helios_checkpoint).await;

        // Get slot head from checkpoint
        helios_client.store.finalized_header.clone().beacon().slot
    }

    pub async fn advance(&mut self) {
        self.job_idx += 1;
        info!("Advance called ready for job {}", self.job_idx);
        if self.next_head > self.current_head {
            info!("Immediately trying to advance {} head", self.bridge_mode.to_string());
            // We should immediately try to process the new head.
            let result = self
                .attempt_finality_update(self.next_head, self.job_idx)
                .await;

            if result.is_err() {
                // If the job failed, print the error and set auto_advance_index to job_idx.
                error!("Error while attempting immediate advance: {:?}. Resorting to auto advance, this will wait for the next finality update opportunity.", result.err());
                self.auto_advance_index = self.job_idx;
            }
        } else {
            info!("Setting flag to attempt auto advance {} head on next finality update.", self.bridge_mode.to_string());

            // We should wait for the next detected head update.
            self.auto_advance_index = self.job_idx;
        }
    }

    pub async fn run(&mut self) {
        loop {
            info!("In run iteration");
            match self.check_finality_next_head().await {
                Ok(next_head) => {
                    // If we have not evolved skip
                    if next_head <= self.current_head {
                        info!(
                            "Nori {} bridge is up to date.",
                            self.bridge_mode.to_string()
                        );
                    } else {
                        // Here if self.next_head is new_head then should not print
                        // We have a new head
                        info!(
                            "Nori {} bridge is stale. Setting next head.",
                            self.bridge_mode.to_string()
                        );
                        self.next_head = next_head;
                        if self.auto_advance_index != 0 {
                            info!("Auto advancing {} head", self.bridge_mode.to_string());
                            // Invoke attempt_finality_update
                            let result = self
                                .attempt_finality_update(self.next_head, self.job_idx)
                                .await;

                            if result.is_err() {
                                // If the job failed, print the error and set auto_advance_index to job_idx.
                                error!("Error while attempting auto advance: {:?}. Will retry next finality slot change", result.err());
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error checking finality slot head: {}", e);
                }
            }

            // Sleep for the specified delay (in seconds)
            let delay_duration = Duration::from_secs_f64(self.polling_interval_sec);
            info!(
                "Sleeping {} {}",
                self.polling_interval_sec,
                if self.polling_interval_sec == 1.0 {
                    "second."
                } else {
                    "seconds."
                }
            );
            sleep(delay_duration).await;
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

    async fn attempt_finality_update(&mut self, slot: u64, job_idx: u64) -> Result<()> {
        self.working_head = slot; // Mark the working head as what was given by the slot

        info!("{} head updater recieved a new job {}. Spawning a new worker.", self.bridge_mode.to_string(), job_idx);

        let last_next_sync_committee = self.next_sync_committee;

        // Attempt async job
        let handle = Handle::current();
        let spawn_result: std::result::Result<
            std::result::Result<SP1ProofWithPublicValues, anyhow::Error>,
            task::JoinError,
        > = task::spawn_blocking(move || {
            handle.block_on(finality_update_job(slot, last_next_sync_committee))
        })
        .await;

        info!("Worker {} done", job_idx);

        // Flatten the nested result into a single Result:
        let task_result: Result<SP1ProofWithPublicValues> = spawn_result
            .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {}", e)) // Map JoinError into anyhow::Error
            .and_then(|inner| inner); // If the inner result is an error, that error is returned

        // If any error occurred, return it.
        if task_result.is_err() {
            return task_result.map(|_| ()); // Propagate the error as a Result<()>.
        }

        info!("Retrieved valid proof from worker {}", job_idx);

        // Otherwise, we have a valid proof.
        if let Ok(proof) = task_result {
            // Check if our result is still relevant after the computation, if our working_head has advanced then we are working on a more recent head in another thread.
            if self.working_head == slot {
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
                self.current_sync_commitee = proof_outputs.sync_committee_hash;

                // Save the checkpoint
                self.save_nb_checkpoint();

                /*
                   Need to carefully deactivate auto_advance, but lets think about this....
                   We might have had advance called multiple times we need some concept of the job number to know if we should prevent auto advancement.
                   If this task was the last auto advance task we should cancel that behaviour
                */
                if job_idx >= self.auto_advance_index { // gt to account for if a previous job spawned failed with an error and didnt cancel itself
                    info!("Cancelling auto advance for job {}", self.auto_advance_index);
                    self.auto_advance_index = 0;
                }

                // Emit output
                match self
                    .proof_dispatcher
                    .trigger(NoriBridgeHeadProofMessage {
                        slot,
                        proof,
                        execution_state_root: proof_outputs.execution_state_root,
                    })
                    .await
                {
                    Ok(_) => {
                        info!("Called all proof listeners succesfully.");
                    }
                    Err(e) => {
                        // If there's an error, print it.
                        error!("Error while triggering proof dispatcher: {:?}", e);
                    }
                }
            }
        }

        Ok(())
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
            current_sync_commitee: self.current_sync_commitee,
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

/*async fn get_helios_client(slot: u64) {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(slot).await; // This panics FIXME

    // Re init helios client
    get_client(helios_checkpoint).await; // This panics FIXME
}*/
