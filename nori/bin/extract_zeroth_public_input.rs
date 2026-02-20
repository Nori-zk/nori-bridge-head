use alloy_primitives::U256;
use anyhow::Result;
use helios_consensus_core::consensus_spec::MainnetConsensusSpec;
use helios_ethereum::rpc::http_rpc::HttpRpc;
use nori::{rpcs::consensus::ConsensusHttpProxy, sp1_prover::finality_update_job, sp1_prover_config::ProverConfig};
use std::{env, fs, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let consensus_client = ConsensusHttpProxy::<MainnetConsensusSpec, HttpRpc>::try_from_env();
    let (current_slot, store_hash) = consensus_client
        .get_latest_finality_slot_and_store_hash()
        .await
        .expect("Expected to get the latest finality slot and store hash");
    let proof_inputs_with_window = consensus_client
        .prepare_consensus_mpt_proof_inputs(current_slot, store_hash, false)
        .await
        .expect("Expected to get proof inputs with a window");

    // Get the mock config
    let config = Arc::new(
        ProverConfig::mock_plonk()
    );
    
    // Run mock program.
    println!("Running SP1 prover");
    let proof_outputs = finality_update_job(config, 0, current_slot, proof_inputs_with_window.proof_inputs)
        .await
        .expect("Expected to run a finality update job");

    // Extract the public input we need.
    let proof_result = proof_outputs.proof();
    let plonk_proof = proof_result.proof.try_as_plonk().expect("Expected a plonk sp1 proof");
    let zeroth_public_input = &plonk_proof.public_inputs[0];
    println!(
        "Extracted plonk sp1Proof.proof.public_inputs[0] {}",
        zeroth_public_input
    );

    // Determine the current project directory (where Cargo.toml is located).
    let project_dir = env::current_dir().expect("Failed to get current directory");
    let cargo_dir = project_dir
        .parent()
        .expect("Failed to find project root directory");

    // Use the correct relative path based on the project root.
    let nori_elf_dir = cargo_dir.join("nori-elf");
    let elf_path = nori_elf_dir.join("nori-sp1-helios-program");
    let output_path = elf_path.with_extension("pi0.json");

    // In sp1-sdk 6.0.1 public_inputs[0] changed from decimal to "FFBn254Fr(0x<hex>)".
    // Downstream needs the canonical decimal field element representation.
    let decimal_pi0 = if let Some(hex) = zeroth_public_input
        .strip_prefix("FFBn254Fr(0x")
        .and_then(|s| s.strip_suffix(')'))
    {
        U256::from_str_radix(hex, 16).expect("invalid hex in public_input").to_string()
    } else {
        zeroth_public_input.clone()
    };

    // Construct the json string from the public input.
    let json_string = format!("\"{}\"", decimal_pi0);

    // Write the file.
    println!("Attempting to write to {:?}", output_path);
    fs::write(&output_path, json_string).expect("Failed to write pi0 JSON file");
    println!("plonk pi0 written to {:?}", output_path);

    Ok(())
}
