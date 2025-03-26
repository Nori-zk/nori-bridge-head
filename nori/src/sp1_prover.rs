use std::sync::OnceLock;
use crate::helios::{get_checkpoint, get_client, get_finality_updates};
use alloy_primitives::{FixedBytes, B256};
use anyhow::{Error, Result};
use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use tree_hash::TreeHash;

pub const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf");

// Cache the proving key globally (initialized once)
static PROVING_KEY: OnceLock<SP1ProvingKey> = OnceLock::new();

async fn initialize_proving_key() -> &'static SP1ProvingKey {
    PROVING_KEY.get_or_init(|| {
        // Initialize fresh client just for setup
        let client = ProverClient::from_env();
        let (pk, _) = client.setup(ELF);
        pk
    })
}

// Struct for ProverJobOutput
pub struct ProverJobOutput {
    job_idx: u64,
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

    pub fn job_idx(&self) -> u64 {
        self.job_idx
    }
}

/// Generates a ZK proof for a finality update at the given slot
///
/// # Arguments
/// * `slot` - Target slot number to prove
/// * `last_next_sync_committee` -  The previous hash of next_sync_committee
pub async fn finality_update_job(
    job_idx: u64,
    input_head: u64,
    last_next_sync_committee: FixedBytes<32>,
) -> Result<ProverJobOutput> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_head).await?;

    // Re init helios client
    let mut helios_update_client = get_client(helios_checkpoint).await?;

    // Get finality update
    info!("Getting finality update.");
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
                .next_sync_committee
                .tree_hash_root()
                .as_ref(),
        );

        if last_next_sync_committee == next_sync_committee {
            info!("Applying optimization, skipping sync committee update.");
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
    };

    // Encode proof inputs
    // TODO make a peristant thread pool
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;

    // Get proving key
    let pk = initialize_proving_key().await;

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup prover client
            info!("Setting up prover client.");
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);
            let prover_client = ProverClient::from_env();

            // Generate proof.
            info!("Running sp1 proof.");
            let proof = prover_client.prove(pk, &stdin).plonk().run();

            proof // Explicitly return proof
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {
        proof,
        input_head,
        job_idx,
    })
}
