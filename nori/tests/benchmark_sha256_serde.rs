use alloy_primitives::FixedBytes;
use anyhow::Result;
use helios_consensus_core::{consensus_spec::MainnetConsensusSpec};
use helios_ethereum::rpc::http_rpc::HttpRpc;
use nori::{
     rpcs::consensus::ConsensusHttpProxy,
    sp1_prover::ELF,
};
use sp1_sdk::{ProverClient, SP1PublicValues, SP1Stdin};

/// A stripped down version of finality_update_job used to generate cycle information without the opt
pub async fn benchmark_finality_update(
    input_head: u64,
    store_hash: FixedBytes<32>,
) -> Result<SP1PublicValues> {
    let consensus_client = ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env();

    let proof_inputs_with_window = consensus_client
        .prepare_consensus_mpt_proof_inputs(input_head, store_hash, false)
        .await
        .unwrap();

    // Encode proof inputs
    println!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = serde_cbor::to_vec(&proof_inputs_with_window)?;

    let public_values = tokio::task::spawn_blocking(move || -> Result<SP1PublicValues> {
        // Setup prover client
        println!("Setting up prover client");
        let mut stdin = SP1Stdin::new();
        stdin.write_slice(&encoded_proof_inputs);
        let prover_client = ProverClient::from_env();
        println!("Prover client setup complete.");

        // Generate report
        let (sp1_public_values, report) = prover_client
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

        Ok(sp1_public_values)
    })
    .await??;

    Ok(public_values)
}

#[tokio::test]
async fn benchmark_sha256_serde() {
    // Get latest head slot and store_hash
    let consensus_client = ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env();
    let (current_slot, store_hash) = consensus_client
        .get_latest_finality_slot_and_store_hash()
        .await
        .unwrap();

    // Perform benchmark initial
    println!("Round one sin opt");
    let public_values = benchmark_finality_update(current_slot, store_hash)
        .await
        .unwrap();
}
