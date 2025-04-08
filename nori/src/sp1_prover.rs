use crate::helios::{get_checkpoint, get_client, get_finality_updates};
use alloy_primitives::{FixedBytes, B256};
use anyhow::{Error, Result};
use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use std::sync::OnceLock;
use tree_hash::TreeHash;

// Import nori sp1 helios program
pub const ELF: &[u8] = include_bytes!("../../nori-elf/nori-sp1-helios-program");

// Cache the proving key globally (initialized once)
static PROVING_KEY: OnceLock<SP1ProvingKey> = OnceLock::new();

pub async fn get_proving_key() -> &'static SP1ProvingKey {
    PROVING_KEY.get_or_init(|| {
        // Initialize fresh client just for setup
        let client = ProverClient::from_env();
        let (pk, _) = client.setup(ELF);
        pk
    })
}

// Struct for ProverJobOutput
pub struct ProverJobOutput {
    job_id: u64,
    input_head: u64,
    proof: SP1ProofWithPublicValues,
}

impl ProverJobOutput {
    pub fn input_head(&self) -> u64 {
        self.input_head
    }

    pub fn proof(&self) -> SP1ProofWithPublicValues {
        self.proof.clone()
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }
}

/// Encodes a prepared helio store ready for a sp1 helio slot transition proof
/// # Arguments
/// * `input_head` - Target slot number to prove from up until current finality head
/// * `last_next_sync_committee` -  The previous hash of next_sync_committee
/// * `store_hash` - The previous hash of the helio client store state at the `input_head` slot
pub async fn prepare_zk_program_input(
    input_head: u64,
    last_next_sync_committee: FixedBytes<32>,
    store_hash: FixedBytes<32>,
) -> Result<Vec<u8>> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_head).await?;

    // Re init helios client
    let mut helios_update_client = get_client(helios_checkpoint).await?;

    // Get finality update
    info!("Getting finality update from input_head {}", input_head);
    let finality_update = helios_update_client
        .rpc
        .get_finality_update()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

    // Get sync commitee updates
    info!("Getting sync commitee updates.");
    let mut sync_committee_updates = get_finality_updates(&helios_update_client).await?;

    // Taken from operator.rs
    // Optimization:
    // Skip processing update inside program if next_sync_committee is already stored in contract.
    // We must still apply the update locally to "sync" the helios client, this is due to
    // next_sync_committee not being stored when the helios client is bootstrapped.
    if !sync_committee_updates.is_empty() {
        let next_sync_committee = B256::from_slice(
            sync_committee_updates[0]
                .next_sync_committee()
                .tree_hash_root()
                .as_ref(),
        );

        if last_next_sync_committee == next_sync_committee {
            info!("Applying optimization, skipping sync committee update. Sync committee hash: {:?}", next_sync_committee);
            let temp_update = sync_committee_updates.remove(0);

            helios_update_client
                .verify_update(&temp_update)
                .map_err(|e| Error::msg(format!("Proof invalid: {}", e)))?; // FIXME what to do with this!
            helios_update_client.apply_update(&temp_update);
        }
    }

    // Create program inputs
    info!("Building sp1 proof inputs.");
    let expected_current_slot = helios_update_client.expected_current_slot();
    let inputs = ProofInputs {
        sync_committee_updates,
        finality_update,
        expected_current_slot,
        store: helios_update_client.store.clone(),
        genesis_root: helios_update_client.config.chain.genesis_root,
        forks: helios_update_client.config.forks.clone(),
        store_hash,
    };
    info!("Built sp1 proof inputs.");

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
    info!("Encoded sp1 proof inputs.");
    Ok(encoded_proof_inputs)
}

/// Generates a ZK proof for a finality update at the given slot
///
/// # Arguments
/// * `job_id` - The identifier for this job
/// * `input_head` - Target slot number to prove from up until current finality head
/// * `last_next_sync_committee` -  The previous hash of next_sync_committee
/// * `store_hash` - The previous hash of the helio client store state at the `input_head` slot
pub async fn finality_update_job(
    job_id: u64,
    input_head: u64,
    last_next_sync_committee: FixedBytes<32>,
    store_hash: FixedBytes<32>,
) -> Result<ProverJobOutput> {
    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = prepare_zk_program_input(input_head, last_next_sync_committee, store_hash).await?;

    // Get proving key
    let pk = get_proving_key().await;

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup prover client
            info!("Setting up prover client");
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);
            let prover_client = ProverClient::from_env();
            info!("Prover client setup complete.");

            // Generate proof.
            info!("Running sp1 proof.");
            let proof = prover_client.prove(pk, &stdin).plonk().run();
            info!("Finished sp1 proof.");

            proof
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_id,
    })
}
