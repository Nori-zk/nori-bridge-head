use alloy_primitives::{FixedBytes, B256};use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use sp1_helios_primitives::types::ProofInputs;
use sp1_helios_script::{get_checkpoint, get_client};
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1Stdin};
use anyhow::Result;
use tree_hash::TreeHash;
use crate::utils::get_finality_updates;

const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf");

pub async fn finality_update_job(
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

    let mut stdin = SP1Stdin::new();
    stdin.write_slice(&encoded_proof_inputs);

    // Generate proof.
    info!("Running sp1 proof.");
    // Note to self do this in a blocking thread....

    // Get prover client and pk
    let prover_client = ProverClient::from_env();
    let (pk, _) = prover_client.setup(ELF);
    let proof = prover_client.prove(&pk, &stdin).plonk().run()?;

    Ok(proof)
}
