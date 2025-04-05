use alloy_primitives::FixedBytes;
use anyhow::Result;
use log::info;
use nori::{helios::get_latest_finality_head_and_store_hash, sp1_prover::{prepare_zk_program_input, ELF}};
use sp1_sdk::{ProverClient, SP1Stdin};

/// A stripped down version of finality_update_job used to generate cycle information without the opt
pub async fn benchmark_finality_update(
    input_head: u64,
    last_next_sync_committee: FixedBytes<32>,
    store_hash: FixedBytes<32>,
) -> Result<()> {

    // Encode proof inputs
    info!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = prepare_zk_program_input(input_head, last_next_sync_committee, store_hash).await?;

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
    let (current_head, store_hash) = get_latest_finality_head_and_store_hash().await.unwrap();

    // Perform benchmark
    benchmark_finality_update(current_head, FixedBytes::default(), store_hash).await.unwrap();
}