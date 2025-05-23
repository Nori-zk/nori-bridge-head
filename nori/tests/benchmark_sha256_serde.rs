use alloy_primitives::FixedBytes;
use anyhow::Result;
use nori::{helios::get_latest_finality_slot_and_store_hash, proof_outputs_decoder::DecodedProofOutputs, sp1_prover::{prepare_zk_program_input, ELF}};
use sp1_sdk::{ProverClient, SP1PublicValues, SP1Stdin};

/// A stripped down version of finality_update_job used to generate cycle information without the opt
pub async fn benchmark_finality_update(
    input_head: u64,
    store_hash: FixedBytes<32>,
) -> Result<SP1PublicValues> {

    // Encode proof inputs
    println!("Encoding sp1 proof inputs.");
    let encoded_proof_inputs = prepare_zk_program_input(input_head, store_hash).await?;

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
    dotenv::dotenv().ok();

    // Get latest head slot and store_hash
    let (current_slot, store_hash) = get_latest_finality_slot_and_store_hash().await.unwrap();

    // Perform benchmark initial
    println!("Round one sin opt");
    let public_values = benchmark_finality_update(current_slot, store_hash).await.unwrap();

    // Decode public output
    let public_values_bytes = public_values.as_slice(); 
    let proof_outputs = DecodedProofOutputs::from_abi(public_values_bytes).unwrap();
    let new_slot = proof_outputs.new_head;
    let new_slot_bytes: [u8; 32] = new_slot.to_be_bytes();
    // Extract the last 8 bytes (least significant bytes in big-endian)
    let new_slot_u64_bytes: [u8; 8] = new_slot_bytes[24..32]
        .try_into()
        .map_err(|_| anyhow::anyhow!("Failed to extract u64 bytes")).unwrap();
    // Convert the bytes to u64 using big-endian interpretation
    let output_slot = u64::from_be_bytes(new_slot_u64_bytes);

    // Perform benchmark 2nd round
    println!("Round two with opt");
    benchmark_finality_update(output_slot, proof_outputs.store_hash).await.unwrap();
}