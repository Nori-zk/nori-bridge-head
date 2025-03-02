use crate::helios::{get_checkpoint, get_client, get_finality_updates};
use alloy_primitives::{FixedBytes, B256};
use anyhow::{bail, Error, Result};
use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1Stdin};
use tree_hash::TreeHash;

pub const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf");

pub struct ProverJobOutput {
    job_idx: u64,
    slot: u64,
    proof: SP1ProofWithPublicValues,
}

impl ProverJobOutput {
    pub fn slot(&self) -> u64 {
        self.slot
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
    slot: u64,
    last_next_sync_committee: FixedBytes<32>,
) -> Result<ProverJobOutput> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(slot).await?;

    // Re init helios client
    let mut heliod_update_client = get_client(helios_checkpoint).await?;

    // Get finality update
    info!("Getting finality update.");
    let finality_update = heliod_update_client
        .rpc
        .get_finality_update()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch finality update via RPC: {}", e))?;

    // Get sync commitee updates
    info!("Getting sync commitee updates.");
    let mut sync_committee_updates = get_finality_updates(&heliod_update_client).await?;

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

            heliod_update_client
                .verify_update(&temp_update)
                .map_err(|e| Error::msg(format!("Proof invalid: {}", e)))?; // FIXME what to do with this!
            heliod_update_client.apply_update(&temp_update);
        }
    }

    // Create program inputs
    info!("Building sp1 proof inputs.");

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
    // TODO make a peristant thread pool
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;

    let proof: SP1ProofWithPublicValues =
        tokio::task::spawn_blocking(move || -> Result<SP1ProofWithPublicValues> {
            // Setup prover client
            info!("Setting up prover client.");
            let mut stdin = SP1Stdin::new();
            stdin.write_slice(&encoded_proof_inputs);
            let prover_client = ProverClient::from_env();
            let (pk, _) = prover_client.setup(ELF); // FIXME this is expensive! What about a persistant thread pool.

            // Generate proof.
            info!("Running sp1 proof.");
            let proof = prover_client.prove(&pk, &stdin).plonk().run();

            proof // Explicitly return proof
        })
        .await??; // Await the blocking task and propagate errors properly

    Ok(ProverJobOutput {proof, slot, job_idx})
}
