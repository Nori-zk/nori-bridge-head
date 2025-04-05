use alloy_primitives::{FixedBytes, B256};
use anyhow::{Error, Result};
use helios_ethereum::rpc::ConsensusRpc;
use log::info;
use nori::{helios::{get_checkpoint, get_client, get_finality_updates, get_latest_finality_head_and_store_hash}, sp1_prover::ELF};
use nori_sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1Stdin};
use tree_hash::TreeHash;

/// A stripped down version of finality_update_job used to generate cycle information
pub async fn benchmark_finality_update_job(
    input_head: u64,
    last_next_sync_committee: FixedBytes<32>,
    store_hash: FixedBytes<32>,
) -> Result<()> {
    // Get latest beacon checkpoint
    let helios_checkpoint = get_checkpoint(input_head).await?;

    // Re init helios client
    let mut helios_update_client = get_client(helios_checkpoint).await?;

    // Get finality update
    info!("Getting finality update from slot {}", input_head);
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
            info!(
                "Applying optimization, skipping sync committee update. Sync committee hash: {:?}",
                next_sync_committee
            );
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

    // Encode proof inputs
    // TODO make a peristant thread pool
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;

    tokio::task::spawn_blocking(move || -> Result<()> {
        // Setup prover client
        info!("Setting up prover client");
        let mut stdin = SP1Stdin::new();
        stdin.write_slice(&encoded_proof_inputs);
        let prover_client = ProverClient::from_env();
        println!("Prover client setup complete.");

        // Generate report
        let (_, report) = prover_client
            .execute(ELF, &stdin)
            // .deferred_proof_verification(false)
            .run()
            .expect("executing failed");
        println!(
            "Execution total_instruction_count: {:?}",
            report.total_instruction_count()
        );
        println!(
            "Execution total_syscall_count: {:?}",
            report.total_syscall_count()
        );

        Ok(())
    })
    .await??;

    Ok(())
}


#[tokio::test]
async fn benchmark_sha256_serde() {
    dotenv::dotenv().ok();

    // Get latest head and store_hash
    let (current_head, store_hash, next_sync_committee) = get_latest_finality_head_and_store_hash().await.unwrap();

    // Print next_sync_commitee
    println!("Next sync committee: {}", next_sync_committee);

    // Perform benchmark
    benchmark_finality_update_job(current_head, next_sync_committee, store_hash).await.unwrap();
}