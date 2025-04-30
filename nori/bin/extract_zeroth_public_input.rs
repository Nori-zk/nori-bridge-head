use std::{env, fs};

use anyhow::Result;
use nori::{helios::get_latest_finality_slot_and_store_hash, sp1_prover::finality_update_job};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    // Get latest head slot and store_hash.
    let (current_slot, store_hash) = get_latest_finality_slot_and_store_hash().await.unwrap();

    // Run mock program.
    println!("Running SP1 prover");
    let proof_outputs = finality_update_job(0, current_slot, store_hash)
        .await
        .unwrap();

    // Extract the public input we need.
    let proof_result = proof_outputs.proof();
    let plonk_proof = proof_result.proof.try_as_plonk().unwrap();
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

    // Construct the json string from the public input.
    let json_string = format!("\"{}\"", zeroth_public_input);

    // Write the file.
    println!("Attempting to write to {:?}", output_path);
    fs::write(&output_path, json_string).expect("Failed to write pi0 JSON file");
    println!("plonk pi0 written to {:?}", output_path);

    Ok(())
}
